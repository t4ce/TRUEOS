//! One CPU sink for the visible Dista atlas experiment, after GPU generation.
//! The R8 mask is retained; no geometry or glyph pixels are constructed here.
use crate::{intel::gpgpu::GpgpuMask8Surface, ui4::FrameRgbaView};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CopyStats {
    pub(super) output_pixels: usize,
    pub(super) source_samples: usize,
    pub(super) partial_coverage_pixels: usize,
}

fn extent(pitch: usize, row: usize, height: usize, available: usize) -> Option<usize> {
    if row == 0 || height == 0 || pitch < row { return None; }
    let bytes = pitch.checked_mul(height - 1)?.checked_add(row)?;
    (bytes <= available).then_some(bytes)
}

fn ranges_overlap(start_a: u64, length_a: usize, start_b: u64, length_b: usize) -> Option<bool> {
    let end_a = start_a.checked_add(u64::try_from(length_a).ok()?)?;
    let end_b = start_b.checked_add(u64::try_from(length_b).ok()?)?;
    Some(start_a < end_b && start_b < end_a)
}

fn ink_over_white(coverage: u32) -> [u8; 4] {
    // Output stays opaque and premultiplied. Coverage applies exactly once,
    // including at overlapping source triangles and during the AA resolve.
    let channel = |ink: u32| ((ink * coverage + 255 * (255 - coverage) + 127) / 255) as u8;
    [channel(48), channel(100), channel(216), 255]
}

/// Copy a completed linear R8 mask directly into a writable UI4 RGBA buffer.
/// `sample_scale=1` copies coverage; `2` resolves each 2x2 source box once.
/// Extents must match exactly; clipping and arbitrary resampling are absent.
///
/// The caller must prove that all source GPU writers retired through render
/// target cache writeback, keep the source stable, and exclusively own every
/// destination byte until publication. Earlier destination GPU writes must
/// also have retired. Publish this lease through the ordinary CPU frame path.
pub(super) unsafe fn copy_retired_mask_to_rgba(
    mask: GpgpuMask8Surface,
    destination: FrameRgbaView,
    sample_scale: u32,
) -> Result<CopyStats, &'static str> {
    if !matches!(sample_scale, 1 | 2) || !mask.is_valid() || mask.phys == 0
        || destination.phys == 0 || destination.virt.is_null()
        || destination.width.checked_mul(sample_scale) != Some(mask.width)
        || destination.height.checked_mul(sample_scale) != Some(mask.height)
    { return Err("dista-copy-shape"); }
    let source_row = mask.width as usize;
    let destination_row = (destination.width as usize).checked_mul(4).ok_or("dista-copy-range")?;
    let source_bytes = extent(mask.pitch_bytes as usize, source_row, mask.height as usize, mask.bytes)
        .ok_or("dista-copy-range")?;
    let destination_bytes = extent(destination.pitch as usize, destination_row,
        destination.height as usize, destination.byte_len).ok_or("dista-copy-range")?;
    usize::try_from(mask.phys).map_err(|_| "dista-copy-range")?;
    (destination.virt as usize).checked_add(destination_bytes).ok_or("dista-copy-range")?;
    if ranges_overlap(mask.phys, mask.bytes, destination.phys, destination.byte_len)
        .ok_or("dista-copy-range")?
    { return Err("dista-copy-alias"); }
    let source = crate::phys::phys_to_virt(mask.phys as usize) as *mut u8;
    if source.is_null() || (source as usize).checked_add(source_bytes).is_none() {
        return Err("dista-copy-range");
    }
    let spans = [
        crate::intel::DmaFlushRows::new(source, source_row, mask.pitch_bytes as usize, mask.height as usize),
        crate::intel::DmaFlushRows::new(destination.virt, destination_row,
            destination.pitch as usize, destination.height as usize),
    ];
    // Drop stale initialization/read lines before reading the completed mask
    // or issuing destination stores that may share a line with row padding.
    if !crate::intel::dma_flush_strided_row_spans(&spans) { return Err("dista-copy-cache-prepare"); }
    let factor = sample_scale as usize;
    let mut stats = CopyStats::default();
    for y in 0..destination.height as usize {
        for x in 0..destination.width as usize {
            let offset = y * factor * mask.pitch_bytes as usize + x * factor;
            let coverage = if factor == 1 {
                unsafe { core::ptr::read_volatile(source.add(offset)) as u32 }
            } else {
                let p = unsafe { source.add(offset) };
                let sum = unsafe {
                    u32::from(core::ptr::read_volatile(p))
                        + u32::from(core::ptr::read_volatile(p.add(1)))
                        + u32::from(core::ptr::read_volatile(p.add(mask.pitch_bytes as usize)))
                        + u32::from(core::ptr::read_volatile(p.add(mask.pitch_bytes as usize + 1)))
                };
                (sum + 2) / 4
            };
            let rgba = ink_over_white(coverage);
            unsafe {
                core::ptr::write_unaligned(destination.virt
                    .add(y * destination.pitch as usize + x * 4).cast::<u32>(),
                    u32::from_le_bytes(rgba));
            }
            stats.partial_coverage_pixels += usize::from(coverage != 0 && coverage != 255);
        }
    }
    stats.output_pixels = destination.width as usize * destination.height as usize;
    stats.source_samples = source_row * mask.height as usize;
    if !crate::intel::dma_flush_strided_row_spans(&spans[1..]) {
        return Err("dista-copy-cache-publish");
    }
    Ok(stats)
}
