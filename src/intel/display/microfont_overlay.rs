//! Borrow the retained native probe for the CPU log, never a GPU render target.
//! Normal application planes 0..3 and Spirit's hardware cursor remain separate.

use super::{OverlaySurface, PipeInfo};
use crate::log_os::microfont_console;

const _: () = assert!(microfont_console::PLANE_SLOT == crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT);

pub(super) fn owns_plane(pipe: PipeInfo, slot: usize) -> bool {
    microfont_console::owns_plane(pipe.slot, slot)
}

/// Bootstrap may arm this existing allocation but must not put it in the
/// ordinary overlay pool: that pool clears, resizes and eventually frees pages.
pub(super) fn bootstrap_surface(
    pipe: PipeInfo, slot: usize, width: u32, height: u32,
) -> Option<OverlaySurface> {
    if !owns_plane(pipe, slot) { return None; }
    let surface = microfont_console::surface()?;
    if surface.width != width || surface.height != height { return None; }
    Some(OverlaySurface {
        width, height, pitch_bytes: surface.pitch_bytes, byte_len: surface.byte_len,
        phys: surface.phys, virt: surface.virt as *mut u8, gpu: surface.gpu,
        pipe, plane_slot: slot, buffer_index: 0,
    })
}
