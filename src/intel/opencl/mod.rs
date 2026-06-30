//! Tiny TRUEOS OpenCL runtime seed for Intel GPUs.
//!
//! This module is intentionally not a full Khronos OpenCL implementation yet.
//! It gives the existing offline-compiled Intel GPGPU artifacts an OpenCL-like
//! object model: contexts, buffers, programs, kernels, queues, and backend
//! execution hooks. The first target is `clCreateProgramWithBinary`-style AOT
//! execution over the direct RCS path in `crate::intel::gpgpu`.

#![allow(dead_code, unused_imports)]

pub(crate) mod api;
pub(crate) mod artifact;
pub(crate) mod backend;
pub(crate) mod example;
pub(crate) mod memory;
pub(crate) mod neo_command;
pub(crate) mod neo_dispatch;
pub(crate) mod neo_helpers;
pub(crate) mod queue;
pub(crate) mod registry;
pub(crate) mod types;
pub(crate) mod validation;

pub(crate) use api::{
    KnownKernelInfo, SourceBuildSmoke, trueos_cl_build_program_from_source,
    trueos_cl_known_kernel_count, trueos_cl_known_kernel_info, trueos_cl_known_kernel_uploaded,
    trueos_cl_probe_known_aot_queue, trueos_cl_reload_all_known_kernels,
    trueos_cl_reload_known_kernel, trueos_cl_source_build_smoke, trueos_cl_upload_known_kernel,
    trueos_cl_validate_known_aot_registry, trueos_cl_validate_known_aot_status,
};
pub(crate) use artifact::{
    BuiltProgram, DescriptorField, DescriptorLayout, GpuArtifactProducer, GpuKernelContract,
    KernelArgAccess, KernelArgDesc, KernelArgKind, KernelCallArg, KernelLaunchContract,
    KernelLaunchModel, KernelMetadata, KernelObject, ProgramArtifact, ProgramBinaryKind,
};
pub(crate) use backend::{BackendCaps, BackendCommand, IntelOpenClBackend, UploadedKernelRef};
pub(crate) use memory::{BufferObject, BufferRegistry};
pub(crate) use neo_command::{
    PipeControlArgs, PipelineSelectArgs, PreemptionMode, QueueThrottle, SubmissionStatus,
    TransferDirection, WaitParams, WaitStatus,
};
pub(crate) use neo_dispatch::{SamplerPatchValue, hw_walk_order, pow_const, split_dispatch};
pub(crate) use neo_helpers::{
    DriverModelType, HeapAddressModel, MapOperationType, get_most_significant_set_bit_index,
    is_any_bit_set, is_bit_set, is_field_valid, is_value_set, make_bit_mask, set_bits,
    shift_left_by,
};
pub(crate) use queue::{Command, CommandKind, CommandQueue, EventRecord, EventStatus};
pub(crate) use registry::{KnownAotKernel, KnownKernelRole};
pub(crate) use types::{
    AccessFlags, ClError, ClResult, ContextId, DeviceId, DeviceKind, EventId, KernelId, MemFlags,
    MemId, NdRange, PlatformId, ProgramId, QueueId, QueueProperties,
};
pub(crate) use validation::{
    KnownAotValidationIssue, KnownAotValidationIssueKind, KnownAotValidationReport,
    validate_known_aot_registry, validate_known_aot_status,
};

pub(crate) const TRUEOS_OPENCL_PLATFORM_NAME: &str = "TRUEOS Intel OpenCL AOT Runtime";
pub(crate) const TRUEOS_OPENCL_PROFILE: &str = "EMBEDDED_PROFILE";
pub(crate) const TRUEOS_OPENCL_VERSION: &str = "OpenCL 1.2 TRUEOS-AOT";
