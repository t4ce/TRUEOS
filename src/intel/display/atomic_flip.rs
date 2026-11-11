//! Native TGL plane/scaler updates must cross one double-buffer boundary.
//! TGL Display PRM pp. 206-207; registers part 1 p. 599, part 2 pp. 753/996.
//! The scaler has its own permission bit, independent of PLANE_CTL bit 3.
use super::*;

pub(super) struct Guard {
    dev: crate::intel::Dev,
    previous: u32,
}

impl Guard {
    pub(super) fn release(self) -> u32 {
        let dev = self.dev;
        drop(self);
        // Same-address flips cannot prove opacity/geometry retirement through
        // SURFLIVE alone. Capture a frame boundary after releasing the hold.
        crate::intel::mmio_read(dev, PIPE_FRMCOUNT_A)
    }
}

pub(super) fn frame_advanced(before: Option<u32>, current: u32) -> bool {
    before.is_none_or(|before| current != before)
}

pub(super) fn begin(dev: crate::intel::Dev) -> Result<Option<Guard>, &'static str> {
    if !native_ui4_for_pipe(PIPES[0]) {
        return Ok(None);
    }
    let previous = crate::intel::mmio_read(dev, DOUBLE_BUFFER_CTL);
    if previous & DOUBLE_BUFFER_GLOBAL_DISABLE != 0 {
        return Err("native-double-buffer-already-held");
    }
    // Establish permission before holding updates. Later scaler control writes
    // retain this bit, including the disabled control used during detach.
    for register in [PIPE_SCALER_0_A_CTRL, PIPE_SCALER_1_A_CTRL] {
        let ctl = crate::intel::mmio_read(dev, register);
        crate::intel::mmio_write(dev, register, ctl | PIPE_SCALER_ALLOW_DOUBLE_BUFFER_DISABLE);
    }
    crate::intel::mmio_write(dev, DOUBLE_BUFFER_CTL, previous | DOUBLE_BUFFER_GLOBAL_DISABLE);
    if crate::intel::mmio_read(dev, DOUBLE_BUFFER_CTL) & DOUBLE_BUFFER_GLOBAL_DISABLE == 0 {
        crate::intel::mmio_write(dev, DOUBLE_BUFFER_CTL, previous);
        let _ = crate::intel::mmio_read(dev, DOUBLE_BUFFER_CTL);
        return Err("native-double-buffer-hold-readback");
    }
    Ok(Some(Guard { dev, previous }))
}

impl Drop for Guard {
    fn drop(&mut self) {
        // Restore on success and every early return. Never leave a global
        // update hold behind when a plane/scaler validation fails.
        crate::intel::mmio_write(self.dev, DOUBLE_BUFFER_CTL, self.previous);
        let _ = crate::intel::mmio_read(self.dev, DOUBLE_BUFFER_CTL);
    }
}
