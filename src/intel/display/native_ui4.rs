//! Capacities and compositor source for the adopted Tiger Lake native panel.
//!
//! Only primary buffers use guarded 64 MiB reservations. Linear overlays fit
//! in 32 MiB. Display GGTT and compositor PPGTT remain separate spaces.

use super::{PipeInfo, tgl_ui4_layout};

pub(super) const PRIMARY_BYTES: u64 = tgl_ui4_layout::PRIMARY_SLOT_BYTES;
const PRIMARY_SOURCE_GPU: u64 = tgl_ui4_layout::primary_source_gpu();

const _: () = {
    assert!(crate::r::ui_surface::UI_SURFACE_GPU_LIMIT <= 0x2000_0000);
    assert!(
        PRIMARY_SOURCE_GPU + PRIMARY_BYTES <= crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES
    );
};

/// Set only after native scanout readback succeeds, independently of GPU or
/// UI4 execution readiness.
pub(super) fn active(pipe: PipeInfo) -> bool {
    pipe.slot == 0 && crate::intel::tgl_native_panel::native_scanout_ready()
}

pub(super) fn primary_swap_capacity(pipe: PipeInfo) -> u64 {
    if active(pipe) {
        PRIMARY_BYTES
    } else {
        super::PRIMARY_SWAP_GPU_STRIDE
    }
}

pub(super) fn overlay_capacity(pipe: PipeInfo) -> u64 {
    if active(pipe) {
        tgl_ui4_layout::OVERLAY_SLOT_BYTES
    } else {
        super::OVERLAY_SWAP_GPU_STRIDE
    }
}

pub(super) fn base_gpu(pipe: PipeInfo, desktop_gpu: u64) -> u64 {
    if active(pipe) {
        PRIMARY_SOURCE_GPU
    } else {
        desktop_gpu
    }
}
