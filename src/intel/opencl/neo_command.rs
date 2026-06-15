//! Low-risk Intel NEO command stream definition leaves.

use core::{ffi::c_void, ptr};

// Source: shared/source/command_stream/submission_status.h
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SubmissionStatus {
    Success = 0,
    Failed = 1,
    OutOfMemory = 2,
    OutOfHostMemory = 3,
    Unsupported = 4,
    DeviceUninitialized = 5,
}

// Source: shared/source/command_stream/wait_status.h
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WaitStatus {
    NotReady = 0,
    Ready = 1,
    GpuHang = 2,
}

// Source: shared/source/command_stream/wait_status.h
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WaitParams {
    pub(crate) indefinitely_poll: bool,
    pub(crate) enable_timeout: bool,
    pub(crate) skip_tbx_download: bool,
    pub(crate) wait_timeout: i64,
}

impl WaitParams {
    pub(crate) const fn new(
        indefinitely_poll: bool,
        enable_timeout: bool,
        skip_tbx_download: bool,
        wait_timeout: i64,
    ) -> Self {
        Self {
            indefinitely_poll,
            enable_timeout,
            skip_tbx_download,
            wait_timeout,
        }
    }
}

// Source: shared/source/command_stream/queue_throttle.h
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum QueueThrottle {
    Low = 0,
    Medium = 1,
    High = 2,
}

// Source: shared/source/command_stream/transfer_direction.h
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TransferDirection {
    HostToHost = 0,
    HostToLocal = 1,
    LocalToHost = 2,
    LocalToLocal = 3,
    Remote = 4,
}

impl TransferDirection {
    pub(crate) const fn from_flags(src_local: bool, dst_local: bool, remote_copy: bool) -> Self {
        if remote_copy {
            Self::Remote
        } else if src_local {
            if dst_local {
                Self::LocalToLocal
            } else {
                Self::LocalToHost
            }
        } else if dst_local {
            Self::HostToLocal
        } else {
            Self::HostToHost
        }
    }
}

// Source: shared/source/command_stream/preemption_mode.h
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreemptionMode {
    // Keep in sync with ForcePreemptionMode debug variable.
    Initial = 0,
    Disabled = 1,
    MidBatch = 2,
    ThreadGroup = 3,
    MidThread = 4,
}

// Source: shared/source/helpers/pipe_control_args.h
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PipeControlArgs {
    pub(crate) post_sync_cmd: *mut c_void,
    pub(crate) block_setting_post_sync_properties: bool,
    pub(crate) cs_stall_only: bool,
    pub(crate) disable_cs_stall: bool,
    pub(crate) dc_flush_enable: bool,
    pub(crate) render_target_cache_flush_enable: bool,
    pub(crate) instruction_cache_invalidate_enable: bool,
    pub(crate) texture_cache_invalidation_enable: bool,
    pub(crate) pipe_control_flush_enable: bool,
    pub(crate) vf_cache_invalidation_enable: bool,
    pub(crate) constant_cache_invalidation_enable: bool,
    pub(crate) state_cache_invalidation_enable: bool,
    pub(crate) generic_media_state_clear: bool,
    pub(crate) hdc_pipeline_flush: bool,
    pub(crate) tlb_invalidation: bool,
    pub(crate) compression_control_surface_ccs_flush: bool,
    pub(crate) notify_enable: bool,
    pub(crate) workload_partition_offset: bool,
    pub(crate) amfs_flush_enable: bool,
    pub(crate) un_typed_data_port_cache_flush: bool,
    pub(crate) depth_cache_flush_enable: bool,
    pub(crate) depth_stall_enable: bool,
    pub(crate) protected_memory_disable: bool,
    pub(crate) is_walker_with_profiling_enqueued: bool,
    pub(crate) command_cache_invalidate_enable: bool,
    pub(crate) is_l1_invalidate_required: bool,
    pub(crate) is_l1_flush_required: bool,
}

impl Default for PipeControlArgs {
    fn default() -> Self {
        Self {
            post_sync_cmd: ptr::null_mut(),
            block_setting_post_sync_properties: false,
            cs_stall_only: false,
            disable_cs_stall: false,
            dc_flush_enable: false,
            render_target_cache_flush_enable: false,
            instruction_cache_invalidate_enable: false,
            texture_cache_invalidation_enable: false,
            pipe_control_flush_enable: false,
            vf_cache_invalidation_enable: false,
            constant_cache_invalidation_enable: false,
            state_cache_invalidation_enable: false,
            generic_media_state_clear: false,
            hdc_pipeline_flush: false,
            tlb_invalidation: false,
            compression_control_surface_ccs_flush: false,
            notify_enable: false,
            workload_partition_offset: false,
            amfs_flush_enable: false,
            un_typed_data_port_cache_flush: false,
            depth_cache_flush_enable: false,
            depth_stall_enable: false,
            protected_memory_disable: false,
            is_walker_with_profiling_enqueued: false,
            command_cache_invalidate_enable: false,
            is_l1_invalidate_required: false,
            is_l1_flush_required: false,
        }
    }
}

// Source: shared/source/helpers/pipeline_select_args.h
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PipelineSelectArgs {
    pub(crate) systolic_pipeline_select_mode: bool,
    pub(crate) is_3d_pipeline_required: bool,
    pub(crate) systolic_pipeline_select_support: bool,
}
