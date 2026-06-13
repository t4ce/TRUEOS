use alloc::vec::Vec;

use crate::intel::gpgpu::GpgpuKernelArtifact;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelArgKind {
    Buffer,
    Image1d,
    Image2d,
    Image3d,
    Sampler,
    LocalMemory,
    Scalar,
    Pod,
    Opaque,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpuArtifactProducer {
    IntelIgcOcloc,
    TrueosC4Eu32,
    HandEu32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelArgAccess {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    ByValue,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelLaunchModel {
    NdRange1d,
    NdRange2d,
    Simd16LaneLoop,
    Simd16DescriptorWorklist,
    Simd16TiledDescriptorWorklist,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KernelCallArg<'a> {
    pub(crate) index: u32,
    pub(crate) name: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) kind: KernelArgKind,
    pub(crate) access: KernelArgAccess,
    pub(crate) size_bytes: u32,
    pub(crate) align_bytes: u32,
    pub(crate) binding_slot: Option<u32>,
    pub(crate) payload_dword: Option<u32>,
}

impl<'a> KernelCallArg<'a> {
    pub(crate) const fn buffer(
        index: u32,
        name: &'a str,
        type_name: &'a str,
        access: KernelArgAccess,
        binding_slot: u32,
        payload_dword: u32,
    ) -> Self {
        Self {
            index,
            name,
            type_name,
            kind: KernelArgKind::Buffer,
            access,
            size_bytes: 8,
            align_bytes: 8,
            binding_slot: Some(binding_slot),
            payload_dword: Some(payload_dword),
        }
    }

    pub(crate) const fn value(
        index: u32,
        name: &'a str,
        type_name: &'a str,
        size_bytes: u32,
        align_bytes: u32,
        payload_dword: u32,
    ) -> Self {
        Self {
            index,
            name,
            type_name,
            kind: KernelArgKind::Scalar,
            access: KernelArgAccess::ByValue,
            size_bytes,
            align_bytes,
            binding_slot: None,
            payload_dword: Some(payload_dword),
        }
    }

    pub(crate) const fn value_kind(
        index: u32,
        name: &'a str,
        type_name: &'a str,
        kind: KernelArgKind,
        size_bytes: u32,
        align_bytes: u32,
        payload_dword: u32,
    ) -> Self {
        Self {
            index,
            name,
            type_name,
            kind,
            access: KernelArgAccess::ByValue,
            size_bytes,
            align_bytes,
            binding_slot: None,
            payload_dword: Some(payload_dword),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescriptorField<'a> {
    pub(crate) name: &'a str,
    pub(crate) dword_offset: u32,
    pub(crate) dwords: u32,
}

impl<'a> DescriptorField<'a> {
    pub(crate) const fn new(name: &'a str, dword_offset: u32, dwords: u32) -> Self {
        Self {
            name,
            dword_offset,
            dwords,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescriptorLayout<'a> {
    pub(crate) name: &'a str,
    pub(crate) stride_dwords: u32,
    pub(crate) max_descriptors: Option<u32>,
    pub(crate) fields: &'a [DescriptorField<'a>],
}

impl<'a> DescriptorLayout<'a> {
    pub(crate) const fn new(
        name: &'a str,
        stride_dwords: u32,
        max_descriptors: Option<u32>,
        fields: &'a [DescriptorField<'a>],
    ) -> Self {
        Self {
            name,
            stride_dwords,
            max_descriptors,
            fields,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KernelLaunchContract {
    pub(crate) model: KernelLaunchModel,
    pub(crate) dimensions: u32,
    pub(crate) simd_width: u32,
    pub(crate) descriptors_per_walker: Option<u32>,
    pub(crate) pixels_per_lane: Option<u32>,
    pub(crate) tile_pixels_per_lane: Option<u32>,
    pub(crate) tile_rows: Option<u32>,
}

impl KernelLaunchContract {
    pub(crate) const fn nd_range_1d() -> Self {
        Self {
            model: KernelLaunchModel::NdRange1d,
            dimensions: 1,
            simd_width: 16,
            descriptors_per_walker: None,
            pixels_per_lane: None,
            tile_pixels_per_lane: None,
            tile_rows: None,
        }
    }

    pub(crate) const fn nd_range_2d(pixels_per_lane: Option<u32>) -> Self {
        Self {
            model: KernelLaunchModel::NdRange2d,
            dimensions: 2,
            simd_width: 16,
            descriptors_per_walker: None,
            pixels_per_lane,
            tile_pixels_per_lane: None,
            tile_rows: None,
        }
    }

    pub(crate) const fn descriptor_worklist(descriptors_per_walker: u32) -> Self {
        Self {
            model: KernelLaunchModel::Simd16DescriptorWorklist,
            dimensions: 1,
            simd_width: 16,
            descriptors_per_walker: Some(descriptors_per_walker),
            pixels_per_lane: None,
            tile_pixels_per_lane: None,
            tile_rows: None,
        }
    }

    pub(crate) const fn tiled_descriptor_worklist(
        descriptors_per_walker: u32,
        tile_pixels_per_lane: u32,
        tile_rows: u32,
    ) -> Self {
        Self {
            model: KernelLaunchModel::Simd16TiledDescriptorWorklist,
            dimensions: 1,
            simd_width: 16,
            descriptors_per_walker: Some(descriptors_per_walker),
            pixels_per_lane: None,
            tile_pixels_per_lane: Some(tile_pixels_per_lane),
            tile_rows: Some(tile_rows),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpuKernelContract<'a> {
    pub(crate) name: &'a str,
    pub(crate) source_path: &'a str,
    pub(crate) producer: GpuArtifactProducer,
    pub(crate) target: &'a str,
    pub(crate) entry_text_offset_bytes: u64,
    pub(crate) cross_thread_bytes: u32,
    pub(crate) per_thread_bytes: u32,
    pub(crate) binding_count: u32,
    pub(crate) args: &'a [KernelCallArg<'a>],
    pub(crate) descriptor_layouts: &'a [DescriptorLayout<'a>],
    pub(crate) launch: KernelLaunchContract,
    pub(crate) consumers: &'a [&'a str],
}

impl<'a> GpuKernelContract<'a> {
    pub(crate) const fn indirect_bytes(self) -> u32 {
        self.cross_thread_bytes + self.per_thread_bytes
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KernelArgDesc<'a> {
    pub(crate) index: u32,
    pub(crate) name: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) kind: KernelArgKind,
    pub(crate) size_bytes: u32,
    pub(crate) align_bytes: u32,
}

impl<'a> KernelArgDesc<'a> {
    pub(crate) const fn new(
        index: u32,
        name: &'a str,
        type_name: &'a str,
        kind: KernelArgKind,
        size_bytes: u32,
        align_bytes: u32,
    ) -> Self {
        Self {
            index,
            name,
            type_name,
            kind,
            size_bytes,
            align_bytes,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct KernelMetadata<'a> {
    pub(crate) name: &'a str,
    pub(crate) args: &'a [KernelArgDesc<'a>],
    pub(crate) required_work_group_size: Option<[u32; 3]>,
    pub(crate) work_group_size_hint: Option<[u32; 3]>,
    pub(crate) preferred_work_group_multiple: u32,
    pub(crate) private_mem_bytes: u32,
    pub(crate) local_mem_bytes: u32,
    pub(crate) gpgpu_artifact: Option<&'a GpgpuKernelArtifact>,
}

impl<'a> KernelMetadata<'a> {
    pub(crate) const fn new(name: &'a str, args: &'a [KernelArgDesc<'a>]) -> Self {
        Self {
            name,
            args,
            required_work_group_size: None,
            work_group_size_hint: None,
            preferred_work_group_multiple: 0,
            private_mem_bytes: 0,
            local_mem_bytes: 0,
            gpgpu_artifact: None,
        }
    }

    pub(crate) const fn with_gpgpu_artifact(
        name: &'a str,
        args: &'a [KernelArgDesc<'a>],
        gpgpu_artifact: &'a GpgpuKernelArtifact,
    ) -> Self {
        Self {
            name,
            args,
            required_work_group_size: None,
            work_group_size_hint: None,
            preferred_work_group_multiple: 0,
            private_mem_bytes: 0,
            local_mem_bytes: 0,
            gpgpu_artifact: Some(gpgpu_artifact),
        }
    }

    pub(crate) const fn arg_count(&self) -> usize {
        self.args.len()
    }

    pub(crate) fn arg(&self, index: u32) -> Option<&KernelArgDesc<'a>> {
        self.args.iter().find(|arg| arg.index == index)
    }

    pub(crate) fn arg_by_name(&self, name: &str) -> Option<&KernelArgDesc<'a>> {
        self.args.iter().find(|arg| arg.name == name)
    }

    pub(crate) fn has_arg_count(&self, count: usize) -> bool {
        self.arg_count() == count
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProgramBinaryKind {
    IntelGenBinary,
    SpirV,
    LlvmBitcode,
    OpenClSource,
    Unknown,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ProgramArtifact<'a> {
    pub(crate) name: &'a str,
    pub(crate) target: &'a str,
    pub(crate) binary_kind: ProgramBinaryKind,
    pub(crate) binary: &'a [u8],
    pub(crate) binary_sha256: Option<[u8; 32]>,
    pub(crate) spirv: Option<&'a [u8]>,
    pub(crate) source: Option<&'a str>,
    pub(crate) build_options: &'a str,
    pub(crate) kernels: &'a [KernelMetadata<'a>],
    pub(crate) gpgpu_artifact: Option<&'a GpgpuKernelArtifact>,
}

impl<'a> ProgramArtifact<'a> {
    pub(crate) const fn new(
        name: &'a str,
        target: &'a str,
        binary_kind: ProgramBinaryKind,
        binary: &'a [u8],
        kernels: &'a [KernelMetadata<'a>],
    ) -> Self {
        Self {
            name,
            target,
            binary_kind,
            binary,
            binary_sha256: None,
            spirv: None,
            source: None,
            build_options: "",
            kernels,
            gpgpu_artifact: None,
        }
    }

    pub(crate) const fn from_gpgpu_kernel(
        artifact: &'a GpgpuKernelArtifact,
        kernels: &'a [KernelMetadata<'a>],
    ) -> Self {
        Self {
            name: artifact.name,
            target: artifact.target,
            binary_kind: ProgramBinaryKind::IntelGenBinary,
            binary: artifact.bin,
            binary_sha256: Some(artifact.bin_sha256),
            spirv: Some(artifact.spv),
            source: None,
            build_options: "",
            kernels,
            gpgpu_artifact: Some(artifact),
        }
    }

    pub(crate) const fn kernel_count(&self) -> usize {
        self.kernels.len()
    }

    pub(crate) fn find_kernel(&self, name: &str) -> Option<&KernelMetadata<'a>> {
        self.kernels.iter().find(|kernel| kernel.name == name)
    }

    pub(crate) fn has_kernel(&self, name: &str) -> bool {
        self.find_kernel(name).is_some()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BuiltProgram<'a> {
    pub(crate) artifact: &'a ProgramArtifact<'a>,
    pub(crate) kernels: Vec<KernelObject<'a>>,
}

impl<'a> BuiltProgram<'a> {
    pub(crate) fn new(artifact: &'a ProgramArtifact<'a>, kernels: Vec<KernelObject<'a>>) -> Self {
        Self { artifact, kernels }
    }

    pub(crate) fn from_artifact(artifact: &'a ProgramArtifact<'a>) -> Self {
        let mut kernels = Vec::with_capacity(artifact.kernels.len());
        for metadata in artifact.kernels {
            kernels.push(KernelObject::new(artifact, metadata));
        }
        Self { artifact, kernels }
    }

    pub(crate) fn kernel_count(&self) -> usize {
        self.kernels.len()
    }

    pub(crate) fn find_kernel(&self, name: &str) -> Option<&KernelObject<'a>> {
        self.kernels.iter().find(|kernel| kernel.name() == name)
    }

    pub(crate) fn has_kernel(&self, name: &str) -> bool {
        self.find_kernel(name).is_some()
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct KernelObject<'a> {
    pub(crate) program: &'a ProgramArtifact<'a>,
    pub(crate) metadata: &'a KernelMetadata<'a>,
    pub(crate) gpgpu_artifact: Option<&'a GpgpuKernelArtifact>,
}

impl<'a> KernelObject<'a> {
    pub(crate) const fn new(
        program: &'a ProgramArtifact<'a>,
        metadata: &'a KernelMetadata<'a>,
    ) -> Self {
        Self {
            program,
            metadata,
            gpgpu_artifact: metadata.gpgpu_artifact,
        }
    }

    pub(crate) const fn name(&self) -> &'a str {
        self.metadata.name
    }

    pub(crate) const fn arg_count(&self) -> usize {
        self.metadata.arg_count()
    }

    pub(crate) fn arg(&self, index: u32) -> Option<&KernelArgDesc<'a>> {
        self.metadata.arg(index)
    }

    pub(crate) fn arg_by_name(&self, name: &str) -> Option<&KernelArgDesc<'a>> {
        self.metadata.arg_by_name(name)
    }
}
