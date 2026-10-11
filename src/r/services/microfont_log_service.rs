//! Laptop IP verdict: one centered Yes/No, 4x microfont, frozen until reboot.
//! CPU-only writer on the retained Slot4 surface; no TCP log consumption.

use spin::Once;

pub(crate) const PLANE_SLOT: usize = 4;
const SCALE: usize = 4;
const ASSIGNMENT_WAIT_MS: u64 = 30_000;
const PERIOD_MS: u64 = 25;
const FG: u32 = 0xFF00_0000;
const BG: u32 = 0xFFFF_FFFF;
static SURFACE: Once<Surface> = Once::new();

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
    *SURFACE.call_once(|| {
        // End the four-colour proof; keep the monitor blank until the verdict.
        unsafe {
            clear_surface(surface);
        }
        crate::intel::dma_cache_flush_range(surface.virt as *const u8, surface.byte_len);
        surface
    }) == surface
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
    let width = text.len() * microfont::FWIDTH * SCALE;
    let height = microfont::FHEIGHT * SCALE;
    let left = (surface.width as usize - width) / 2;
    let top = (surface.height as usize - height) / 2;
    for (column, &byte) in text.iter().enumerate() {
        let mut glyph = [BG; microfont::FWIDTH * microfont::FHEIGHT];
        let _ = microfont::stamp_bytes(
            &mut glyph,
            microfont::FWIDTH,
            microfont::FHEIGHT,
            0,
            0,
            &[byte],
            FG,
        );
        for y in 0..height {
            for x in 0..microfont::FWIDTH * SCALE {
                let color = glyph[(y / SCALE) * microfont::FWIDTH + x / SCALE];
                let offset = (top + y) * surface.pitch_bytes as usize
                    + (left + column * microfont::FWIDTH * SCALE + x) * 4;
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
    let Some(surface) = surface() else {
        return;
    };
    // USB discovery can arrive after this task starts. Wait without taking
    // network/driver locks; neither other NICs nor TCP log bytes decide Yes.
    let started = trueos_time::Instant::now().as_millis();
    loop {
        if let Some(assigned) = crate::net::laptop_profile::assignment_result() {
            paint_verdict(surface, assigned);
            return; // Freeze this one line and retain plane ownership.
        }
        if trueos_time::Instant::now()
            .as_millis()
            .saturating_sub(started)
            >= ASSIGNMENT_WAIT_MS
        {
            paint_verdict(surface, false);
            return; // A later adapter registration cannot repaint the verdict.
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(PERIOD_MS)).await;
    }
}
