//! Camera bring-up ABI v1: a frozen 64x64 RGBA tile, never a live DMA buffer.
use alloc::vec::Vec;
use core::sync::atomic::{Ordering, fence};
use trueos_time::{Duration, Timer};

#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraStatus {
    pub flags: u32,
    pub width: u16,
    pub height: u16,
    pub frames: u32,
    pub edid_reads: u32,
}

pub(crate) fn status() -> Result<CameraStatus, &'static str> {
    let guard = super::TGA.lock();
    let dev = guard.as_ref().ok_or("FPGA offline")?;
    if !super::is_present(dev) { return Err("FPGA disappeared"); }
    let read = |offset| super::Tga::read_reg(dev.mmio_base + offset);
    if read(0x280) != 0x43414d31 { return Err("camera firmware not loaded"); }
    let dimensions = read(0x288);
    Ok(CameraStatus { flags: read(0x284), width: dimensions as u16,
        height: (dimensions >> 16) as u16, frames: read(0x28c), edid_reads: read(0x290) })
}

pub(crate) async fn read_tile() -> Result<Vec<u8>, &'static str> {
    let state = status()?;
    if state.flags & 4 == 0 { return Err("waiting for HDMI pixels (tile not ready)"); }
    let generation = super::connection_generation();
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for row in 0..64u32 {
        {
            let guard = super::TGA.lock();
            let dev = guard.as_ref().ok_or("FPGA offline")?;
            if generation != super::connection_generation() || !super::is_present(dev) {
                return Err("FPGA changed during capture");
            }
            for col in 0..64u32 {
                super::Tga::write_reg(dev.mmio_base + 0x300, row * 64 + col);
                fence(Ordering::SeqCst);
                // A non-posted read drains the index write and allows the synchronous RAM read to settle.
                let _ = super::Tga::read_reg(dev.mmio_base + 0x284);
                pixels.extend_from_slice(&super::Tga::read_reg(dev.mmio_base + 0x304).to_le_bytes());
            }
        }
        Timer::after(Duration::from_millis(1)).await;
    }
    Ok(pixels)
}
