//! Cold, post-retirement pixel checks for the native Dista target experiment.
//!
//! The timed producer never reads pixels. This diagnostic retains dense byte
//! snapshots after each white-on-zero target's measured burst has retired.
use alloc::vec::Vec;
use crate::intel::gpgpu::{GpgpuMask8Surface, GpgpuRgba8Surface};

const WIDTH: u32 = 1536;
const HEIGHT: u32 = 1056;
const CELL_WIDTH: usize = 96;
const CELL_HEIGHT: usize = 176;
const COLUMNS: usize = 16;
const CELLS: usize = 96;
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Format { Mask, Rgba }

pub(super) struct Snapshot {
    coverage: Vec<u8>,
    format: Format,
    nonbinary_pixels: usize,
    channel_mismatches: usize,
    nonblank_glyphs: usize,
    space_pixels: usize,
    unused_pixels: usize,
    nonzero_pixels: usize,
    coverage_sum: u64,
    hash: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Audit {
    pub(super) checked_pixels: usize,
    pub(super) mismatched_pixels: usize,
    pub(super) nonbinary_pixels: usize,
    pub(super) rgba_nonbinary_pixels: usize,
    pub(super) rgba_channel_mismatches: usize,
    pub(super) nonblank_glyphs: usize,
    pub(super) rgba_nonblank_glyphs: usize,
    pub(super) space_pixels: usize,
    pub(super) rgba_space_pixels: usize,
    pub(super) unused_pixels: usize,
    pub(super) rgba_unused_pixels: usize,
    pub(super) nonzero_pixels: usize,
    pub(super) coverage_sum: u64,
    pub(super) r8_hash: u64,
    pub(super) rgba_hash: u64,
    pub(super) format_mismatches: usize,
}

impl Audit {
    pub(super) fn passed(self) -> bool {
        self.checked_pixels == WIDTH as usize * HEIGHT as usize
            && self.mismatched_pixels == 0 && self.nonbinary_pixels == 0
            && self.rgba_nonbinary_pixels == 0 && self.rgba_channel_mismatches == 0
            && self.nonblank_glyphs == 94 && self.rgba_nonblank_glyphs == 94
            && self.space_pixels == 0 && self.rgba_space_pixels == 0
            && self.unused_pixels == 0 && self.rgba_unused_pixels == 0
            && self.nonzero_pixels != 0 && self.r8_hash == self.rgba_hash
            && self.format_mismatches == 0
    }
}

fn hash_byte(hash: u64, value: u8) -> u64 {
    (hash ^ u64::from(value)).wrapping_mul(FNV_PRIME)
}

fn nonblank_cells(cells: &[usize; CELLS]) -> usize {
    cells[1..95].iter().filter(|&&count| count != 0).count()
}

fn valid_extent(phys: u64, bytes: usize, width: u32, height: u32, pitch: u32, bpp: usize)
    -> bool
{
    phys != 0 && width == WIDTH && height == HEIGHT
        && (width as usize).checked_mul(bpp).is_some_and(|row| row <= pitch as usize)
        && (pitch as usize).checked_mul(height as usize).is_some_and(|size| size <= bytes)
        && phys.checked_add(bytes as u64).is_some()
        && usize::try_from(phys).is_ok()
}

/// The caller must retain exclusive ownership, prove every GPU writer retired
/// through render-target cache writeback, and keep backing stable for this call.
/// This must run after the complete measured burst, never between warm samples.
pub(super) unsafe fn capture_rgba_retired(surface: GpgpuRgba8Surface)
    -> Result<Snapshot, &'static str>
{
    if !surface.is_valid()
        || !valid_extent(surface.phys, surface.bytes, surface.width, surface.height,
            surface.pitch_bytes, 4)
    { return Err("atlas-validation-rgba-shape"); }
    Ok(unsafe { capture_retired(surface.phys, surface.bytes, surface.pitch_bytes, Format::Rgba) })
}

/// Same ownership and retirement requirements as `capture_rgba_retired`.
pub(super) unsafe fn capture_r8_retired(surface: GpgpuMask8Surface)
    -> Result<Snapshot, &'static str>
{
    if !surface.is_valid()
        || !valid_extent(surface.phys, surface.bytes, surface.width, surface.height,
            surface.pitch_bytes, 1)
    { return Err("atlas-validation-r8-shape"); }
    Ok(unsafe { capture_retired(surface.phys, surface.bytes, surface.pitch_bytes, Format::Mask) })
}

unsafe fn capture_retired(phys: u64, bytes: usize, pitch: u32, format: Format) -> Snapshot {
    let base = crate::phys::phys_to_virt(phys as usize) as *const u8;
    // The CPU HHDM may have retained the allocation's initialization lines.
    // Invalidate them only after the ordered GPU writeback has completed.
    crate::intel::dma_cache_flush_range(base, bytes);
    let mut reference = Snapshot {
        coverage: Vec::with_capacity(WIDTH as usize * HEIGHT as usize),
        format, nonbinary_pixels: 0, channel_mismatches: 0, nonblank_glyphs: 0,
        space_pixels: 0, unused_pixels: 0, nonzero_pixels: 0,
        coverage_sum: 0, hash: FNV_OFFSET,
    };
    let mut cells = [0usize; CELLS];
    let bpp = if format == Format::Rgba { 4 } else { 1 };
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let pixel = unsafe { base.add(y * pitch as usize + x * bpp) };
            let red = unsafe { core::ptr::read_volatile(pixel) };
            if format == Format::Rgba {
                let other = [1usize, 2, 3].map(|channel| unsafe {
                    core::ptr::read_volatile(pixel.add(channel))
                });
                reference.channel_mismatches += usize::from(other.iter().any(|&value| value != red));
            }
            reference.nonbinary_pixels += usize::from(red != 0 && red != 255);
            reference.coverage.push(red);
            reference.hash = hash_byte(reference.hash, red);
            reference.coverage_sum += u64::from(red);
            if red != 0 {
                reference.nonzero_pixels += 1;
                cells[(y / CELL_HEIGHT) * COLUMNS + x / CELL_WIDTH] += 1;
            }
        }
    }
    reference.nonblank_glyphs = nonblank_cells(&cells);
    reference.space_pixels = cells[0];
    reference.unused_pixels = cells[95];
    reference
}

/// Exact full-atlas equivalence; CPU snapshots are never GPU pixel sources.
pub(super) fn compare(mask: &Snapshot, rgba: &Snapshot) -> Audit {
    Audit {
        checked_pixels: mask.coverage.len().min(rgba.coverage.len()),
        mismatched_pixels: mask.coverage.iter().zip(&rgba.coverage).filter(|(a, b)| a != b).count(),
        nonbinary_pixels: mask.nonbinary_pixels,
        rgba_nonbinary_pixels: rgba.nonbinary_pixels,
        rgba_channel_mismatches: rgba.channel_mismatches,
        nonblank_glyphs: mask.nonblank_glyphs,
        rgba_nonblank_glyphs: rgba.nonblank_glyphs,
        space_pixels: mask.space_pixels, rgba_space_pixels: rgba.space_pixels,
        unused_pixels: mask.unused_pixels, rgba_unused_pixels: rgba.unused_pixels,
        nonzero_pixels: mask.nonzero_pixels, coverage_sum: mask.coverage_sum,
        r8_hash: mask.hash, rgba_hash: rgba.hash,
        format_mismatches: usize::from(mask.format != Format::Mask)
            + usize::from(rgba.format != Format::Rgba),
    }
}
