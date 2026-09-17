//! Address layout for the one successfully adopted Tiger Lake native panel.
//!
//! The desktop's 16 MiB slots cannot hold 3840x2160, especially the guarded
//! primary. Keep desktop and other-pipe addresses unchanged. Display GGTT and
//! compositor PPGTT are separate address spaces: never pass a high scanout VA
//! to the bounded direct-RCS page table as the primary's source address.

use super::PipeInfo;

pub(super) const PRIMARY_BYTES: u64 = 0x0400_0000;
const OVERLAY_BYTES: u64 = 0x0200_0000;
pub(super) const PRIMARY_GPU: u64 = 0xE400_0000;
const PRIMARY_SWAP_GPU: u64 = 0xE800_0000;
const OVERLAY_GPU: u64 = 0xF000_0000;
const PRIMARY_COMPOSE_GPU: u64 = 0x2000_0000;
const OVERLAY_COMPOSE_GPU: u64 = 0x2800_0000;
const PRIMARY_SOURCE_GPU: u64 = 0x3400_0000;
const BUFFER_COUNT: usize = 2;
const OVERLAY_COUNT: usize = 4;
const COMPOSE_OVERLAY_COUNT: usize = 3;

const _: () = {
    assert!(super::PRIMARY_SWAP_BUFFER_COUNT == BUFFER_COUNT);
    assert!(super::OVERLAY_SWAP_BUFFER_COUNT == BUFFER_COUNT);
    assert!(super::OVERLAY_UNIVERSAL_PLANE_COUNT == OVERLAY_COUNT);
    assert!(super::DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT == COMPOSE_OVERLAY_COUNT);
    // Preserve the CPU proof frame and every existing direct-scanout import.
    assert!(
        super::UI4_DIRECT_SCANOUT_GPU_BASE
            + super::UI4_DIRECT_SCANOUT_PLANE_COUNT as u64
                * super::UI4_DIRECT_SCANOUT_PLANE_STRIDE
            <= crate::intel::tgl_native_panel::SURFACE_GPU
    );
    assert!(
        crate::intel::tgl_native_panel::SURFACE_GPU
            + crate::intel::tgl_native_panel::SURFACE_GPU_CAPACITY
            <= PRIMARY_GPU
    );
    assert!(PRIMARY_GPU + PRIMARY_BYTES <= PRIMARY_SWAP_GPU);
    assert!(PRIMARY_SWAP_GPU + BUFFER_COUNT as u64 * PRIMARY_BYTES <= OVERLAY_GPU);
    assert!(
        OVERLAY_GPU + OVERLAY_COUNT as u64 * BUFFER_COUNT as u64 * OVERLAY_BYTES
            <= 0x1_0000_0000
    );
    // Only destinations/base use these aliases in the compositor's PPGTT.
    // Published UI-surface producers retain their addresses below this range.
    assert!(crate::r::ui_surface::UI_SURFACE_GPU_LIMIT <= PRIMARY_COMPOSE_GPU);
    assert!(PRIMARY_COMPOSE_GPU + BUFFER_COUNT as u64 * PRIMARY_BYTES <= OVERLAY_COMPOSE_GPU);
    assert!(
        OVERLAY_COMPOSE_GPU + COMPOSE_OVERLAY_COUNT as u64 * BUFFER_COUNT as u64 * OVERLAY_BYTES
            <= PRIMARY_SOURCE_GPU
    );
    assert!(
        PRIMARY_SOURCE_GPU + PRIMARY_BYTES <= crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES
    );
};

/// This immutable boot fact is set only after native scanout readback succeeds.
/// It does not assert GuC, RCS, UI4 plane-stack or artifact readiness.
pub(super) fn active(pipe: PipeInfo) -> bool {
    pipe.slot == 0 && crate::intel::tgl_native_panel::native_scanout_ready()
}

pub(super) fn primary_swap_gpu(index: usize) -> Option<u64> {
    if index >= BUFFER_COUNT {
        return None;
    }
    Some(PRIMARY_SWAP_GPU + index as u64 * PRIMARY_BYTES)
}

pub(super) fn overlay_gpu(slot: usize, index: usize) -> Option<u64> {
    if !(1..=OVERLAY_COUNT).contains(&slot) || index >= BUFFER_COUNT {
        return None;
    }
    Some(OVERLAY_GPU + ((slot - 1) * BUFFER_COUNT + index) as u64 * OVERLAY_BYTES)
}

pub(super) fn compose_gpu(slot: usize, index: usize) -> Option<u64> {
    if index >= BUFFER_COUNT {
        return None;
    }
    if slot == 0 {
        return Some(PRIMARY_COMPOSE_GPU + index as u64 * PRIMARY_BYTES);
    }
    if slot > COMPOSE_OVERLAY_COUNT {
        return None;
    }
    Some(OVERLAY_COMPOSE_GPU + ((slot - 1) * BUFFER_COUNT + index) as u64 * OVERLAY_BYTES)
}

pub(super) fn primary_swap_capacity(pipe: PipeInfo) -> u64 {
    if active(pipe) { PRIMARY_BYTES } else { super::PRIMARY_SWAP_GPU_STRIDE }
}

pub(super) fn overlay_capacity(pipe: PipeInfo) -> u64 {
    if active(pipe) { OVERLAY_BYTES } else { super::OVERLAY_SWAP_GPU_STRIDE }
}

pub(super) fn compose_capacity(pipe: PipeInfo, slot: usize) -> u64 {
    if !active(pipe) {
        super::COMPOSE_RCS_GPU_ALIAS_BYTES
    } else if slot == 0 {
        PRIMARY_BYTES
    } else {
        OVERLAY_BYTES
    }
}

pub(super) fn base_gpu(pipe: PipeInfo, desktop_gpu: u64) -> u64 {
    if active(pipe) { PRIMARY_SOURCE_GPU } else { desktop_gpu }
}
