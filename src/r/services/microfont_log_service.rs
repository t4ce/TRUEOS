//! Live laptop IP verdict and USB bring-up stage on the retained text surface.
//! CPU-only writer on the retained Slot4 surface; no TCP log consumption.

use spin::Once;

pub(crate) const PLANE_SLOT: usize = 4;
const SCALE: usize = 4;
const PERIOD_MS: u64 = 25;
const BG: u32 = 0xFFFF_FFFF;
static SURFACE: Once<Surface> = Once::new();
// Synchronous publication works even before the background executor runs.
static STATUS: spin::Mutex<(&'static str, Option<bool>, bool)> =
    spin::Mutex::new(("USB service not started", None, false));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Surface {
    pub phys: u64,
    pub gpu: u64,
    pub virt: usize,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
}

impl Surface {
    fn valid(self) -> bool {
        let Some(row_bytes) = (self.width as usize).checked_mul(4) else {
            return false;
        };
        let Some(bytes) = (self.pitch_bytes as usize).checked_mul(self.height as usize) else {
            return false;
        };
        self.virt != 0
            && self.virt % 4 == 0
            && self.phys != 0
            && self.phys % 4096 == 0
            && self.gpu % 4096 == 0
            && self.pitch_bytes % 4 == 0
            && self.pitch_bytes as usize >= row_bytes
            && bytes <= self.byte_len
            && self.byte_len <= isize::MAX as usize
            && self.virt.checked_add(self.byte_len).is_some()
            && self.width as usize >= 3 * SCALE * microfont::FWIDTH
            && self.height as usize >= SCALE * microfont::FHEIGHT
    }
}

/// Boot-only handoff, before AP registration and normal UI4 bootstrap.
/// # Safety
/// The caller permanently retains this mapped allocation. After this call,
/// only this service may write it; display may read it but no GPU may render
/// into it, and no other owner may clear, resize, free or reuse it.
pub(crate) unsafe fn install(surface: Surface) -> bool {
    if !surface.valid() {
        return false;
    }
    let installed = *SURFACE.call_once(|| {
        let mut status = STATUS.lock();
        status.1 = crate::net::laptop_profile::assignment_result();
        repaint(surface, *status);
        surface
    }) == surface;
    // Close the race with stage/configuration callbacks before Once publication.
    if installed {
        let mut status = STATUS.lock();
        status.1 = crate::net::laptop_profile::assignment_result();
        repaint(surface, *status);
    }
    installed
}

/// Called synchronously by NIC configuration; independent of AP/task timers.
pub(crate) fn assignment_changed(assigned: bool) {
    let mut status = STATUS.lock();
    if status.1 != Some(assigned) {
        status.1 = Some(assigned);
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

/// Publish the operation before awaiting hardware; a stalled transfer leaves its
/// exact stage visible. Never takes a NIC lock or depends on task timers.
pub(crate) fn usb_stage(stage: &'static str) {
    let mut status = STATUS.lock();
    status.2 = true;
    if status.0 != stage {
        status.0 = stage;
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

/// Once the adapter is found, another controller must not hide its progress.
pub(crate) fn usb_controller_stage(stage: &'static str) {
    let mut status = STATUS.lock();
    if !status.2 && status.0 != stage {
        status.0 = stage;
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

pub(crate) fn font_ready() {
    let status = STATUS.lock();
    if let Some(surface) = surface() { repaint(surface, *status); }
}

fn repaint(surface: Surface, status: (&str, Option<bool>, bool)) {
    unsafe {
        clear_surface(surface);
    }
    paint_verdict(surface, status.1.unwrap_or(false));
    // Fit the status below the verdict, including on small diagnostic surfaces.
    let top = (surface.height as usize + microfont::FHEIGHT * SCALE) / 2;
    let scale = SCALE
        .min(surface.width as usize / (status.0.len().max(1) * microfont::FWIDTH))
        .min((surface.height as usize - top) / microfont::FHEIGHT);
    if scale > 0 {
        paint_text(surface, status.0.as_bytes(), scale, top);
    }
    crate::intel::dma_cache_flush_range(surface.virt as *const u8, surface.byte_len);
}

pub(crate) fn enabled() -> bool {
    SURFACE.get().is_some()
}
pub(crate) fn surface() -> Option<Surface> {
    SURFACE.get().copied()
}
pub(crate) fn owns_plane(pipe: usize, slot: usize) -> bool {
    pipe == 0 && slot == PLANE_SLOT && enabled()
}

/// Fill visible pixels only; preserve pitch padding and allocation guards.
unsafe fn clear_surface(surface: Surface) {
    for y in 0..surface.height as usize {
        for x in 0..surface.width as usize {
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt + y * surface.pitch_bytes as usize + x * 4) as *mut u32,
                    BG,
                );
            }
        }
    }
}

fn paint_verdict(surface: Surface, assigned: bool) {
    let text: &[u8] = if assigned { b"Yes" } else { b"No" };
    let top = (surface.height as usize - microfont::FHEIGHT * SCALE) / 2;
    paint_text(surface, text, SCALE, top);
}

fn paint_text(surface: Surface, text: &[u8], scale: usize, top: usize) {
    let width = text.len() * microfont::FWIDTH * scale;
    let height = microfont::FHEIGHT * scale;
    let left = (surface.width as usize - width) / 2;
    for (column, &byte) in text.iter().enumerate() {
        let Some(atlas) = super::microfont_mask_service::ready_atlas() else { return; };
        let ch = microfont::glyph_character(byte).unwrap_or(' ');
        for y in 0..height {
            for x in 0..microfont::FWIDTH * scale {
                let alpha = u32::from(atlas.coverage(ch, x as u32, y as u32,
                    (microfont::FWIDTH * scale) as u32, height as u32));
                let color = 0xff00_0000 | ((255 - alpha) * 0x010101);
                let offset = (top + y) * surface.pitch_bytes as usize
                    + (left + column * microfont::FWIDTH * scale + x) * 4;
                unsafe {
                    core::ptr::write_volatile((surface.virt + offset) as *mut u32, color);
                }
            }
        }
    }
    let _ = crate::intel::dma_flush_strided_rows(
        (surface.virt + top * surface.pitch_bytes as usize + left * 4) as *mut u8,
        width * 4,
        surface.pitch_bytes as usize,
        height,
    );
}

#[cfg(not(test))]
#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_log_task(assigned_slot: u32) {
    if crate::cpu::CpuProfile::current().map(|cpu| cpu.slot()) != Some(assigned_slot) {
        return;
    }
    let Some(_surface) = surface() else {
        return;
    };
    // Keep the IP result live: a slow discovery must never freeze a false No.
    loop {
        if let Some(assigned) = crate::net::laptop_profile::assignment_result() {
            assignment_changed(assigned);
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(PERIOD_MS)).await;
    }
}
