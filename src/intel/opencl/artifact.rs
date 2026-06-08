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
