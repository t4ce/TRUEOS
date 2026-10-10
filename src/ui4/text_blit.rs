//! Small synchronous operations on exclusively owned, fully retired RGBA8.
//! BCS0 remains the large-work path. Selection never bypasses its owner proofs.
use crate::intel::{DmaFlushRows, GucBcs0MonoGlyph, GucBcs0RgbaCopy, GucBcs0RgbaSurface};
use alloc::vec::Vec;

pub(crate) fn stamp() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        core::arch::x86_64::_rdtsc()
    }
}
pub(crate) fn nanos(ticks: u64) -> u64 {
    ((ticks as u128 * 1_000_000_000) / crate::time::tsc_hz() as u128) as u64
}

fn valid(s: GucBcs0RgbaSurface, x: u32, y: u32, w: u32, h: u32) -> bool {
    s.phys != 0
        && w != 0
        && h != 0
        && x.checked_add(w).is_some_and(|r| r <= s.width)
        && y.checked_add(h).is_some_and(|b| b <= s.height)
        && (s.width as usize)
            .checked_mul(4)
            .is_some_and(|bytes| bytes <= s.pitch_bytes as usize)
        && (s.pitch_bytes as usize)
            .checked_mul(s.height as usize)
            .is_some_and(|bytes| bytes <= s.bytes)
        && s.phys.checked_add(s.bytes as u64).is_some()
}
fn base(s: GucBcs0RgbaSurface) -> *mut u8 {
    crate::phys::phys_to_virt(s.phys as usize) as *mut u8
}
fn span(s: GucBcs0RgbaSurface, x: u32, y: u32, w: u32, h: u32) -> DmaFlushRows {
    DmaFlushRows::new(
        unsafe { base(s).add(y as usize * s.pitch_bytes as usize + x as usize * 4) },
        w as usize * 4,
        s.pitch_bytes as usize,
        h as usize,
    )
}
pub(crate) fn mono_pixels(glyphs: &[GucBcs0MonoGlyph]) -> usize {
    glyphs
        .iter()
        .fold(0usize, |sum, g| sum.saturating_add(g.width as usize * g.height as usize))
}
pub(crate) fn copy_pixels(copies: &[GucBcs0RgbaCopy]) -> usize {
    copies
        .iter()
        .fold(0usize, |sum, c| sum.saturating_add(c.width as usize * c.height as usize))
}

/// Caller owns every destination byte and proves all earlier GPU writes retired.
/// Sources for copy operations additionally remain stable through this call.
pub(crate) unsafe fn mono(s: GucBcs0RgbaSurface, glyphs: &[GucBcs0MonoGlyph]) -> bool {
    if glyphs.is_empty()
        || glyphs
            .iter()
            .any(|g| g.width > 16 || g.height > 32 || !valid(s, g.x, g.y, g.width, g.height))
    {
        return false;
    }
    let spans: Vec<_> = glyphs
        .iter()
        .map(|g| span(s, g.x, g.y, g.width, g.height))
        .collect();
    // Invalidate stale cache lines before partial-line stores, then publish only
    // touched rows with one final fence. Never flush the complete display frame.
    if !crate::intel::dma_flush_strided_row_spans(&spans) {
        return false;
    }
    for g in glyphs {
        for y in 0..g.height as usize {
            let row = unsafe {
                base(s).add((g.y as usize + y) * s.pitch_bytes as usize + g.x as usize * 4)
            };
            for x in 0..g.width as usize {
                let ink = g.mask[y * 2 + x / 8] & (0x80 >> (x % 8)) != 0;
                unsafe {
                    core::ptr::write_unaligned(
                        row.add(x * 4).cast::<u32>(),
                        if ink { g.foreground } else { g.background },
                    );
                }
            }
        }
    }
    crate::intel::dma_flush_strided_row_spans(&spans)
}

pub(crate) unsafe fn copy(destination: GucBcs0RgbaSurface, copies: &[GucBcs0RgbaCopy]) -> bool {
    if copies.is_empty()
        || copies.iter().any(|c| {
            !valid(destination, c.destination_x, c.destination_y, c.width, c.height)
                || !valid(c.source, c.source_x, c.source_y, c.width, c.height)
                || (c.source.phys < destination.phys.saturating_add(destination.bytes as u64)
                    && destination.phys < c.source.phys.saturating_add(c.source.bytes as u64))
        })
    {
        return false;
    }
    let source_spans: Vec<_> = copies
        .iter()
        .map(|c| span(c.source, c.source_x, c.source_y, c.width, c.height))
        .collect();
    let spans: Vec<_> = copies
        .iter()
        .map(|c| span(destination, c.destination_x, c.destination_y, c.width, c.height))
        .collect();
    if !crate::intel::dma_flush_strided_row_spans(&source_spans)
        || !crate::intel::dma_flush_strided_row_spans(&spans)
    {
        return false;
    }
    for c in copies {
        for y in 0..c.height as usize {
            let source = unsafe {
                base(c.source).add(
                    (c.source_y as usize + y) * c.source.pitch_bytes as usize
                        + c.source_x as usize * 4,
                )
            };
            let target = unsafe {
                base(destination).add(
                    (c.destination_y as usize + y) * destination.pitch_bytes as usize
                        + c.destination_x as usize * 4,
                )
            };
            unsafe {
                core::ptr::copy_nonoverlapping(source, target, c.width as usize * 4);
            }
        }
    }
    crate::intel::dma_flush_strided_row_spans(&spans)
}

pub(crate) unsafe fn try_mono(s: GucBcs0RgbaSurface, glyphs: &[GucBcs0MonoGlyph]) -> bool {
    let pixels = mono_pixels(glyphs);
    let half_row = glyphs
        .first()
        .map_or(0, |g| s.width as usize * g.height as usize / 2);
    pixels != 0
        && pixels <= crate::allcaps::text_blit::CPU_MONO_MAX_PIXELS.min(half_row)
        && unsafe { mono(s, glyphs) }
}
pub(crate) unsafe fn try_copy(
    s: GucBcs0RgbaSurface,
    copies: &[GucBcs0RgbaCopy],
    row_height: u32,
) -> bool {
    let pixels = copy_pixels(copies);
    pixels != 0
        && pixels
            <= crate::allcaps::text_blit::CPU_COPY_MAX_PIXELS
                .min(s.width as usize * row_height as usize / 2)
        && unsafe { copy(s, copies) }
}
pub(crate) fn report(kind: &str, pixels: usize, cpu: bool, batches: usize, start: u64) {
    if crate::allcaps::text_blit::DIAGNOSTICS && pixels != 0 {
        let ns = nanos(stamp().wrapping_sub(start));
        crate::log_info!(target: "apps"; "text-blit: kind={} ap={} pixels={} bytes={} backend={} bcs_batches={} wall_ns={}\n",
            kind, crate::percpu::current_slot(), pixels, pixels*4, if cpu {"cpu"} else {"bcs0"}, batches, ns);
    }
}

pub(crate) async fn benchmark_once() -> Result<(), &'static str> {
    super::text_blit_bench::run_once().await
}
