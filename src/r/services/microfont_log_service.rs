//! Laptop bring-up: the TCP log's bytes, painted once by the final AP.
//! No independent sink policy, producer-side rasterization, scroll or GPU work.
//! Reuse the retained native proof allocation as the CPU-only Slot4 front.
//! Fill left top-to-bottom, then right; clip line tails until the next newline.

use core::fmt::{self, Write};
use spin::Once;

pub(crate) const PLANE_SLOT: usize = 4;
const TEXT_COLUMNS: usize = 2;
const BATCH_BYTES: usize = 4096;
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
            && self.width as usize >= TEXT_COLUMNS * microfont::FWIDTH
            && self.height as usize >= 2 * microfont::FHEIGHT
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
        // End the four-colour proof. Unused overlay pixels expose the existing
        // pipe bottom colour, rather than introducing another background.
        unsafe {
            core::ptr::write_bytes(surface.virt as *mut u8, 0, surface.byte_len);
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

struct Pen {
    surface: Surface,
    columns: usize,
    rows: usize,
    text_column: usize,
    row: usize,
    column: usize,
    dirty_start: usize,
    dirty_end: usize,
}

impl Pen {
    fn new(surface: Surface) -> Self {
        Self {
            surface,
            columns: surface.width as usize / TEXT_COLUMNS / microfont::FWIDTH,
            rows: surface.height as usize / microfont::FHEIGHT - 1,
            text_column: 0,
            row: 0,
            column: 0,
            dirty_start: usize::MAX,
            dirty_end: 0,
        }
    }
    fn full(&self) -> bool {
        self.row >= self.rows
            || (self.text_column == TEXT_COLUMNS - 1
                && self.row == self.rows - 1
                && self.column == self.columns)
    }
    fn pixel_offset(&self, column: usize) -> usize {
        self.row * microfont::FHEIGHT * self.surface.pitch_bytes as usize
            + (self.text_column * (self.surface.width as usize / TEXT_COLUMNS)
                + column * microfont::FWIDTH)
                * 4
    }
    fn flush(&mut self) {
        if self.dirty_start >= self.dirty_end {
            return;
        }
        let offset = self.pixel_offset(self.dirty_start);
        let _ = crate::intel::dma_flush_strided_rows(
            (self.surface.virt + offset) as *mut u8,
            (self.dirty_end - self.dirty_start) * microfont::FWIDTH * 4,
            self.surface.pitch_bytes as usize,
            microfont::FHEIGHT,
        );
        self.dirty_start = usize::MAX;
        self.dirty_end = 0;
    }
    fn newline(&mut self) {
        self.flush();
        self.row += 1;
        self.column = 0;
        if self.row == self.rows && self.text_column + 1 < TEXT_COLUMNS {
            self.text_column += 1;
            self.row = 0;
        }
    }
    fn glyph(&mut self, byte: u8) {
        // The crate's packed font table is the cache. One 264-byte temporary
        // preserves microfont's exact glyph semantics, including its q bias.
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
        for y in 0..microfont::FHEIGHT {
            let offset = self.pixel_offset(self.column) + y * self.surface.pitch_bytes as usize;
            for x in 0..microfont::FWIDTH {
                unsafe {
                    core::ptr::write_volatile(
                        (self.surface.virt + offset + x * 4) as *mut u32,
                        glyph[y * microfont::FWIDTH + x],
                    );
                }
            }
        }
        self.dirty_start = self.dirty_start.min(self.column);
        self.column += 1;
        self.dirty_end = self.column;
    }
    fn byte(&mut self, byte: u8) {
        if self.full() {
            return;
        }
        match byte {
            b'\n' => self.newline(),
            b'\r' => {} // Never overwrite already displayed evidence.
            b'\t' => {
                for _ in 0..(4 - self.column % 4) {
                    self.byte(b' ');
                }
            }
            0..=31 | 127 => {}
            byte => {
                // One input line owns one half-width row. Drop its tail, even
                // across consumer batches; only a newline advances the pen.
                if self.column < self.columns {
                    self.glyph(if byte.is_ascii() { byte } else { b'?' });
                }
            }
        }
    }
    fn finish(&mut self) {
        self.flush();
        // The single status row still spans the screen, below both columns.
        self.text_column = 0;
        self.row = self.rows;
        self.column = 0;
        for &byte in b"SCREEN FULL - frozen; TCP logging and the kernel continue"
            .iter()
            .take(self.surface.width as usize / microfont::FWIDTH)
        {
            self.glyph(byte);
        }
        self.flush();
    }
}

impl Write for Pen {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if self.full() {
                return Err(fmt::Error);
            }
            self.byte(byte);
        }
        Ok(())
    }
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
    let mut pen = Pen::new(surface);
    let _ = writeln!(
        pen,
        "TCP LOG MIRROR | last AP {} | microfont 1x | 2 cols left->right | clip/no scroll",
        assigned_slot
    );
    pen.flush();
    // No log! calls or driver-status queries here: a stuck producer/driver
    // must not prevent this independent reader from painting existing bytes.
    let mut cursor = 0u64;
    let mut batch = [0u8; BATCH_BYTES];
    loop {
        // copy_for_screen releases the shared ring lock before any font/pixel
        // work. It never consumes bytes belonging to the network reader.
        if let Some((count, lost)) =
            crate::log_os::logtotcp::copy_for_screen(&mut cursor, &mut batch)
        {
            if lost != 0 {
                let _ = writeln!(
                    pen,
                    "[screen] {} bytes overwritten before this reader caught up",
                    lost
                );
            }
            for &byte in &batch[..count] {
                pen.byte(byte);
            }
            pen.flush();
        }
        if pen.full() {
            pen.finish();
            return; // Freeze pixels; keep their ownership until reboot.
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(PERIOD_MS)).await;
    }
}
