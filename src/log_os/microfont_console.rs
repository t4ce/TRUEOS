//! Temporary 9A49/r01 bring-up console: one page, no scroll, no GPU writer.
//!
//! Capture starts before GT/GuC initialization. Cells are retained in static RAM
//! until the native-panel allocation is available. That same allocation is then
//! the CPU-only interaction-plane front; normal UI4/Spirit still run beneath it.
//! All emitted levels reach this sink independently of the TCP/UART policies.
//! Explicitly disabled log callsites and once/rate-limit suppression stay disabled.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use log_os_core::{GlobalLogSink, GlobalLogSinkSpec, LogArea, LogAreaSet, LogLevel,
    LogLevelFilter, LogLevelPolicy};
use spin::{Mutex, Once};

pub(crate) const ENABLED: bool = true;
pub(crate) const PLANE_SLOT: usize = 4;
const WIDTH: usize = 3840;
const HEIGHT: usize = 2160;
const COLUMNS: usize = WIDTH / microfont::FWIDTH;
// Keep one final row for an explicit FULL indicator, not another log record.
const LOG_ROWS: usize = HEIGHT / microfont::FHEIGHT - 1;
const CELLS: usize = COLUMNS * LOG_ROWS;
const FG: u32 = 0xFFFF_FFFF;
const BG: u32 = 0xFF00_0000;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static FULL: AtomicBool = AtomicBool::new(false);
static CONTENDED: AtomicUsize = AtomicUsize::new(0);
static PAGE: Mutex<Page<CELLS>> = Mutex::new(Page::new(COLUMNS, LOG_ROWS));
static SURFACE: Once<Surface> = Once::new();

/// CPU-owned, process-lifetime backing. No GPU context may write these pages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) virt: usize,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

impl Surface {
    fn valid_for(self, columns: usize, rows: usize) -> bool {
        let Some(row_bytes) = (self.width as usize).checked_mul(4) else { return false; };
        let Some(bytes) = (self.pitch_bytes as usize).checked_mul(self.height as usize) else { return false; };
        self.virt != 0 && self.virt % 4 == 0 && self.phys != 0
            && self.phys % 4096 == 0 && self.gpu % 4096 == 0
            && self.pitch_bytes % 4 == 0 && self.pitch_bytes as usize >= row_bytes
            && bytes <= self.byte_len && self.byte_len <= isize::MAX as usize
            && self.virt.checked_add(self.byte_len).is_some()
            && columns.checked_mul(microfont::FWIDTH).is_some_and(|w| w <= self.width as usize)
            && rows.checked_add(1).and_then(|r| r.checked_mul(microfont::FHEIGHT))
                .is_some_and(|h| h <= self.height as usize)
    }
}

pub(crate) fn enable_for_pci_identity(identity: u32, revision: u8) {
    if !ENABLED || identity != 0x9A49_8086 || revision != 0x01 {
        return;
    }
    if !ACTIVE.swap(true, Ordering::AcqRel) {
        // Called once on the BSP before GT initialization, not from a log path.
        let mut page = PAGE.lock();
        let _ = writeln!(page, "TGL CPU LOG v1 | microfont 1x 6x11 | {} columns x {} log rows | first records retained, no scrolling", COLUMNS, LOG_ROWS);
    }
}

pub(crate) fn surface() -> Option<Surface> { SURFACE.get().copied() }

// Full means stop writing, NOT release the display plane or its backing.
pub(crate) fn owns_plane(pipe_slot: usize, plane_slot: usize) -> bool {
    pipe_slot == 0 && plane_slot == PLANE_SLOT && SURFACE.get().is_some()
}

/// Attach only after the caller has permanently retained a successfully mapped
/// native-panel allocation. Replays earlier GT/GuC messages without any GPU work.
///
/// # Safety
/// `surface` must describe mapped CPU-writable memory retained until reboot,
/// with exclusive CPU write ownership transferred here. Scanout may read it;
/// no other CPU or GPU producer may write, free or repurpose it afterwards.
pub(crate) unsafe fn attach(surface: Surface) -> bool {
    if !ACTIVE.load(Ordering::Acquire) || !surface.valid_for(COLUMNS, LOG_ROWS) {
        return false;
    }
    if let Some(existing) = SURFACE.get() { return *existing == surface; }
    let Some(mut page) = PAGE.try_lock() else { return false; };
    if let Some(existing) = SURFACE.get() { return *existing == surface; }
    // Transparent unused pixels preserve the UI underneath after the handoff.
    // During the initial XRGB primary phase, the same zeros are simply black.
    unsafe { core::ptr::write_bytes(surface.virt as *mut u8, 0, surface.byte_len); }
    crate::intel::dma_cache_flush_range(surface.virt as *const u8, surface.byte_len);
    page.surface = Some(surface);
    page.rendered = 0;
    page.render_pending();
    SURFACE.call_once(|| surface);
    true
}

pub(super) struct ScreenLogSink;
pub(super) static SINK: ScreenLogSink = ScreenLogSink;

impl GlobalLogSink for ScreenLogSink {
    fn spec(&self) -> GlobalLogSinkSpec {
        GlobalLogSinkSpec::new(LogAreaSet::ALL, LogLevelPolicy::up(LogLevelFilter::Trace))
    }
    fn accepts(&self, _area: LogArea, _level: LogLevel) -> bool {
        ACTIVE.load(Ordering::Acquire) && !FULL.load(Ordering::Acquire)
    }
    fn write_accepted(&self, area: LogArea, _level: LogLevel, purpose: Option<&str>, args: fmt::Arguments<'_>) {
        if !self.accepts(area, _level) { return; }
        // IRQ/re-entrant/concurrent logging must not wait on its interrupted
        // writer. Keep this separate from the existing TCP/UART locks.
        let Some(mut page) = PAGE.try_lock() else {
            CONTENDED.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if page.full { return; }
        let skipped = CONTENDED.swap(0, Ordering::Relaxed);
        if skipped != 0 {
            let _ = writeln!(page, "[screen] {} records dropped during concurrent/re-entrant logging", skipped);
        }
        let _ = write!(page, "[{}] ", log_os_core::area_tag(area));
        if let Some(purpose) = purpose { let _ = write!(page, "[{}] ", purpose); }
        let _ = page.write_fmt(args);
        if page.column != 0 { let _ = page.write_str("\n"); }
        page.render_pending();
        FULL.store(page.full, Ordering::Release);
    }
}

struct Page<const N: usize> {
    cells: [u8; N],
    columns: usize,
    rows: usize,
    row: usize,
    column: usize,
    rendered: usize,
    full: bool,
    footer_full: Option<bool>,
    surface: Option<Surface>,
}

impl<const N: usize> Page<N> {
    const fn new(columns: usize, rows: usize) -> Self {
        assert!(columns > 0 && rows > 0 && columns * rows == N);
        Self { cells: [0; N], columns, rows, row: 0, column: 0,
            rendered: 0, full: false, footer_full: None, surface: None }
    }
    fn newline(&mut self) {
        self.row += 1;
        self.column = 0;
        self.full = self.row >= self.rows;
    }
    fn put(&mut self, byte: u8) -> fmt::Result {
        if self.full { return Err(fmt::Error); }
        if self.column == self.columns { self.newline(); }
        if self.full { return Err(fmt::Error); }
        self.cells[self.row * self.columns + self.column] = byte;
        self.column += 1;
        self.full = self.row == self.rows - 1 && self.column == self.columns;
        Ok(())
    }
    fn paint_cell(&self, surface: Surface, row: usize, column: usize, byte: u8) {
        let mut glyph = [BG; microfont::FWIDTH * microfont::FHEIGHT];
        if microfont::stamp_bytes(&mut glyph, microfont::FWIDTH, microfont::FHEIGHT,
            0, 0, &[byte], FG).is_err() { return; }
        for y in 0..microfont::FHEIGHT {
            let offset = (row * microfont::FHEIGHT + y) * surface.pitch_bytes as usize
                + column * microfont::FWIDTH * 4;
            for x in 0..microfont::FWIDTH {
                // Bounds were checked once at attach; these cells never extend
                // past the visible width/height or into pitch padding.
                unsafe { core::ptr::write_volatile((surface.virt + offset + x * 4) as *mut u32,
                    glyph[y * microfont::FWIDTH + x]); }
            }
        }
    }
    fn flush_cells(&self, surface: Surface, row: usize, start: usize, end: usize) {
        let offset = row * microfont::FHEIGHT * surface.pitch_bytes as usize
            + start * microfont::FWIDTH * 4;
        let _ = crate::intel::dma_flush_strided_rows((surface.virt + offset) as *mut u8,
            (end - start) * microfont::FWIDTH * 4, surface.pitch_bytes as usize, microfont::FHEIGHT);
    }
    fn render_pending(&mut self) {
        let Some(surface) = self.surface else { return; };
        let end = (self.row * self.columns + self.column).min(N);
        while self.rendered < end {
            let row = self.rendered / self.columns;
            let start_column = self.rendered % self.columns;
            let limit = end.min((row + 1) * self.columns);
            for index in self.rendered..limit {
                let byte = self.cells[index];
                if byte != 0 { self.paint_cell(surface, row, index % self.columns, byte); }
            }
            self.flush_cells(surface, row, start_column, limit - row * self.columns);
            self.rendered = limit;
        }
        if self.footer_full != Some(self.full) {
            let text: &[u8] = if self.full {
                b"SCREEN FULL - further screen records dropped; this does not halt the kernel"
            } else {
                b"CPU LOG - append only; unused pixels transparent; no scrolling or GPU font execution"
            };
            // Footer is the only replaceable row. Existing log cells are never
            // cleared or overwritten, even when a record does not fit.
            for column in 0..self.columns {
                self.paint_cell(surface, self.rows, column, text.get(column).copied().unwrap_or(b' '));
            }
            self.flush_cells(surface, self.rows, 0, self.columns);
            self.footer_full = Some(self.full);
        }
    }
}

impl<const N: usize> Write for Page<N> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for character in text.chars() {
            if self.full { return Err(fmt::Error); }
            match character {
                '\n' => self.newline(),
                '\r' => {}, // Never overwrite an earlier log line.
                '\t' => { for _ in 0..(4 - self.column % 4) { self.put(b' ')?; } },
                c if c.is_control() => {},
                c => self.put(if c.is_ascii() { c as u8 } else { b'?' })?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capacity_is_640_by_195_plus_status_row() {
        assert_eq!((COLUMNS, LOG_ROWS, CELLS), (640, 195, 124800));
        assert!((LOG_ROWS + 1) * microfont::FHEIGHT <= HEIGHT);
    }

    #[test]
    fn exact_wrap_newline_and_full_never_overwrite() {
        let mut page = Page::<8>::new(4, 2);
        page.write_str("abcd\nEFGH").unwrap();
        assert_eq!(&page.cells, b"abcdEFGH");
        assert!(page.full);
        let saved = page.cells;
        assert!(page.write_str("\rREPLACE\n").is_err());
        assert_eq!(page.cells, saved);
        let mut wrap = Page::<8>::new(4, 2);
        wrap.write_str("abcdefgh").unwrap();
        assert_eq!(&wrap.cells, b"abcdefgh");
        assert!(wrap.full);
    }

    #[test]
    fn controls_are_append_only_and_unicode_is_one_fallback_per_scalar() {
        let mut page = Page::<24>::new(8, 3);
        page.write_str("A\rB\tö💡\nC").unwrap();
        assert_eq!(&page.cells[..6], b"AB  ??");
        assert_eq!(page.cells[8], b'C');
        page.write_str("\n\n").unwrap();
        assert!(page.full);
        assert!(page.write_str("later").is_err());
        assert_eq!(&page.cells[..6], b"AB  ??");
    }

    fn fixture(columns: usize, rows: usize) -> (std::vec::Vec<u32>, Surface) {
        let width = columns * microfont::FWIDTH;
        let height = (rows + 1) * microfont::FHEIGHT;
        let stride = width + 8;
        let mut storage = std::vec![0x1234_5678; stride * height + 32];
        let surface = Surface { phys: 0x1000, gpu: 0xE000_0000,
            virt: unsafe { storage.as_mut_ptr().add(16) } as usize,
            byte_len: stride * height * 4, width: width as u32, height: height as u32,
            pitch_bytes: (stride * 4) as u32 };
        assert!(surface.valid_for(columns, rows));
        (storage, surface)
    }

    #[test]
    fn raster_matches_real_microfont_and_preserves_padding_and_canaries() {
        let (mut storage, surface) = fixture(4, 2);
        let mut page = Page::<8>::new(4, 2);
        page.write_str("Aq\nZ").unwrap(); // All of this predates surface attachment.
        page.surface = Some(surface);
        page.render_pending();
        let stride = surface.pitch_bytes as usize / 4;
        let mut expected = [BG; microfont::FWIDTH * microfont::FHEIGHT];
        microfont::stamp_bytes(&mut expected, microfont::FWIDTH, microfont::FHEIGHT,
            0, 0, b"A", FG).unwrap();
        for y in 0..microfont::FHEIGHT {
            for x in 0..microfont::FWIDTH {
                assert_eq!(storage[16 + y * stride + x], expected[y * microfont::FWIDTH + x]);
            }
        }
        for y in 0..surface.height as usize {
            for x in surface.width as usize..stride {
                assert_eq!(storage[16 + y * stride + x], 0x1234_5678);
            }
        }
        assert!(storage[..16].iter().all(|x| *x == 0x1234_5678));
        assert!(storage[storage.len()-16..].iter().all(|x| *x == 0x1234_5678));
        let before = storage.clone();
        page.render_pending();
        assert_eq!(storage, before); // No periodic repaint or scrolling.
        page.write_str("123").unwrap();
        let _ = page.write_str("stop");
        page.render_pending();
        let frozen = storage.clone();
        let _ = page.write_str("never reaches pixels");
        page.render_pending();
        assert_eq!(storage, frozen);
        storage.clear(); // No attached production global in these local tests.
    }

    #[test]
    fn rejects_short_bad_pitch_unaligned_and_overflowing_surfaces() {
        let (_storage, surface) = fixture(4, 2);
        assert!(!Surface { byte_len: 1, ..surface }.valid_for(4, 2));
        assert!(!Surface { pitch_bytes: 4, ..surface }.valid_for(4, 2));
        assert!(!Surface { virt: surface.virt + 1, ..surface }.valid_for(4, 2));
        assert!(!Surface { virt: usize::MAX - 3, ..surface }.valid_for(4, 2));
        assert!(!Surface { height: 1, ..surface }.valid_for(4, 2));
        assert!(!surface.valid_for(usize::MAX, 2));
    }

    #[test]
    fn sink_is_target_only_nonblocking_and_full_does_not_suppress_other_sinks() {
        enable_for_pci_identity(0x4680_8086, 0x0c);
        assert!(!SINK.accepts(LogArea::Gfx, LogLevel::Info));
        enable_for_pci_identity(0x9A49_8086, 0x02);
        assert!(!SINK.accepts(LogArea::Gfx, LogLevel::Info));
        enable_for_pci_identity(0x9A49_8086, 0x01);
        assert!(SINK.accepts(LogArea::Gfx, LogLevel::Trace));
        let guard = PAGE.lock();
        SINK.write_accepted(LogArea::Gfx, LogLevel::Info, None, format_args!("nested"));
        assert_eq!(CONTENDED.load(Ordering::Relaxed), 1);
        drop(guard);
        SINK.write_accepted(LogArea::Gfx, LogLevel::Info, None, format_args!("before attach"));
        assert_eq!(CONTENDED.load(Ordering::Relaxed), 0);
        assert!(surface().is_none());
        FULL.store(true, Ordering::Release);
        assert!(!SINK.accepts(LogArea::Gfx, LogLevel::Error));
        struct Other(AtomicUsize);
        impl GlobalLogSink for Other {
            fn spec(&self) -> GlobalLogSinkSpec { SINK.spec() }
            fn write_accepted(&self, _: LogArea, _: LogLevel, _: Option<&str>, _: fmt::Arguments<'_>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        static OTHER: Other = Other(AtomicUsize::new(0));
        static SINKS: [&'static dyn GlobalLogSink; 2] = [&SINK, &OTHER];
        let router = log_os_core::GlobalLogRouter::new(&SINKS);
        log_os_core::log_with_area_level(&router, LogArea::Gfx, LogLevel::Error, format_args!("still delivered"));
        assert_eq!(OTHER.0.load(Ordering::Relaxed), 1);
    }
}
