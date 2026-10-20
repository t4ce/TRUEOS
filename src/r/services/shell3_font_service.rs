//! Shell3's shared native-line/quad font authority. One R8 production pass is
//! retained for service life; styled RGBA cells are warmed once and copied by
//! either the CPU or legacy BLT. Shell3 owns layout, clipping and sink choice.
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use super::microfont_mask_service::{OwnedAlphaAtlas, build_shell3_alpha_atlas};
use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaSurface, gpgpu::GpgpuOwnedRgba8Surface};

const PAGE_COLUMNS: u32 = 8;
const PAGE_ROWS: u32 = 8;
static FONT: spin::Once<Font> = spin::Once::new();
static PREPARING: AtomicBool = AtomicBool::new(false);
type Reply = Signal<crate::wait::EmbassySpinRawMutex, Result<(), &'static str>>;

struct Font {
    alpha: OwnedAlphaAtlas,
    cache: spin::Mutex<CellCache>,
}
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct CellKey {
    character: char,
    width: u32,
    height: u32,
    foreground: [u8; 4],
    background: [u8; 4],
    underline: bool,
}
struct CellPage {
    // Pages are never recycled: even cancelled or uncertain BLTs retain valid
    // source pixels and GPU aliases for the entire service lifetime.
    backing: GpgpuOwnedRgba8Surface,
    cell_width: u32,
    cell_height: u32,
    used: u32,
}
#[derive(Default)]
struct CellCache {
    cells: BTreeMap<CellKey, GucBcs0RgbaCopy>,
    pages: Vec<CellPage>,
    solids: BTreeMap<(u64, u32, u32), Option<u32>>,
}

/// Call before acquiring a writable frame/cache lease. Concurrent shells share
/// the same initialization; no executor is blocked while the native pass runs.
pub(crate) async fn prepare() -> Result<(), &'static str> {
    loop {
        if FONT.get().is_some() { return Ok(()); }
        if PREPARING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            Timer::after(Duration::from_millis(10)).await;
            continue;
        }
        let reply = alloc::sync::Arc::new(Reply::new());
        let response = alloc::sync::Arc::clone(&reply);
        let job = Box::new(move || {
            let result = prepare_once();
            PREPARING.store(false, Ordering::Release);
            response.signal(result);
        });
        if let Err(job) = crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "shell3-font") {
            drop(job);
            PREPARING.store(false, Ordering::Release);
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        match reply.wait().await {
            Err("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy") => {
                Timer::after(Duration::from_millis(20)).await;
            }
            result => return result,
        }
    }
}

fn prepare_once() -> Result<(), &'static str> {
    if FONT.get().is_some() { return Ok(()); }
    let (_, height) = crate::intel::physical_extent_pixels(5, 5)
        .ok_or("shell3-font-physical-size-unavailable")?;
    let alpha = build_shell3_alpha_atlas(height)?;
    let glyphs = alpha.glyphs().count();
    let line_width_px = alpha.line_width_px;
    FONT.call_once(|| Font { alpha, cache: spin::Mutex::new(CellCache::default()) });
    crate::log_important!(target: "microfont-mask";
        "shell3-font: ready format=r8 em_mm=5 em_height_px={} width_px={} glyphs={} successful_productions=1 retained=service-lifetime layout=unchanged bitmap_fallback=missing-only\n",
        height, line_width_px, glyphs);
    Ok(())
}

/// Returns an immutable, fully coloured cell. Destination coordinates are zero;
/// the caller places/crops the descriptor without repainting its source.
pub(crate) fn glyph(
    character: char,
    cell_width: u32,
    cell_height: u32,
    foreground: [u8; 4],
    background: [u8; 4],
    underline: bool,
) -> Result<GucBcs0RgbaCopy, &'static str> {
    let font = FONT.get().ok_or("shell3-font-not-ready")?;
    if cell_width == 0 || cell_height == 0 || cell_width > 256 || cell_height > 256 {
        return Err("shell3-font-cell-size");
    }
    let key = CellKey { character, width: cell_width, height: cell_height, foreground, background, underline };
    let mut cache = font.cache.lock();
    if let Some(copy) = cache.cells.get(&key) { return Ok(*copy); }
    let page_index = if let Some(index) = cache.pages.iter().position(|page| {
        page.cell_width == cell_width && page.cell_height == cell_height
            && page.used < PAGE_COLUMNS * PAGE_ROWS
    }) { index } else {
        let backing = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(
            cell_width * PAGE_COLUMNS, cell_height * PAGE_ROWS,
        ).ok_or("shell3-font-cell-allocation")?;
        cache.pages.push(CellPage { backing, cell_width, cell_height, used: 0 });
        cache.pages.len() - 1
    };
    let page = &mut cache.pages[page_index];
    let left = (page.used % PAGE_COLUMNS) * cell_width;
    let top = (page.used / PAGE_COLUMNS) * cell_height;
    let s = page.backing.surface();
    let destination = crate::phys::phys_to_virt(s.phys as usize) as *mut u8;
    let native = font.alpha.glyph(character);
    // Preserve non-line characters (Braille, Han, private-use UI cells, and
    // not-yet-minted symbols). This expansion is cached, never per-frame work.
    let fallback = native.is_none().then(|| microfont::glyph_cell_pixels(character));
    let mut first_pixel = None;
    let mut uniform = true;
    for y in 0..cell_height {
        for x in 0..cell_width {
            let mut coverage = if let Some(mask) = native {
                sample_native(&font.alpha, mask, character, x, y, cell_width, cell_height)
            } else {
                sample_bitmap(fallback.unwrap(), character, x, y, cell_width, cell_height)
            };
            if underline && y * microfont::FHEIGHT as u32 / cell_height == microfont::FHEIGHT as u32 - 1 {
                coverage = 255;
            }
            let pixel = tint(coverage, foreground, background);
            if let Some(first) = first_pixel { uniform &= first == pixel; }
            else { first_pixel = Some(pixel); }
            unsafe {
                core::ptr::write_unaligned(destination.add(
                    (top + y) as usize * s.pitch_bytes as usize + (left + x) as usize * 4,
                ).cast::<u32>(), u32::from_le_bytes(pixel));
            }
        }
    }
    // A page contains disjoint immutable cells. New cells never change source
    // bytes held by an in-flight copy, including copies owned by another shell.
    let rows = [crate::intel::DmaFlushRows::new(
        unsafe { destination.add(top as usize * s.pitch_bytes as usize + left as usize * 4) },
        cell_width as usize * 4, s.pitch_bytes as usize, cell_height as usize,
    )];
    if !crate::intel::dma_flush_strided_row_spans(&rows) {
        return Err("shell3-font-cell-publish");
    }
    let copy = GucBcs0RgbaCopy {
        source: GucBcs0RgbaSurface { phys: s.phys, gpu: s.gpu, bytes: s.bytes,
            width: s.width, height: s.height, pitch_bytes: s.pitch_bytes },
        source_x: left, source_y: top, destination_x: 0, destination_y: 0,
        width: cell_width, height: cell_height,
    };
    page.used += 1;
    cache.solids.insert((s.gpu, left, top), if uniform { first_pixel.map(u32::from_le_bytes) } else { None });
    cache.cells.insert(key, copy);
    Ok(copy)
}

/// Warm-time metadata for Shell3's existing solid-fill batching. A clipped
/// subrectangle of a uniform full cell is uniform too; mixed cells stay copies.
pub(crate) fn solid_color(copy: &GucBcs0RgbaCopy) -> Option<u32> {
    FONT.get()?.cache.lock().solids.get(&(copy.source.gpu, copy.source_x, copy.source_y))
        .copied().flatten()
}

fn sample_native(atlas: &OwnedAlphaAtlas, mask: &[u8], character: char,
    x: u32, y: u32, width: u32, height: u32) -> u8 {
    let quad = ('\u{2500}'..='\u{259f}').contains(&character);
    let content_width = (atlas.em_height_px * microfont::FWIDTH as u32).div_ceil(microfont::FHEIGHT as u32);
    // The quad cell fills the old cell completely so adjacent box/block cells
    // meet. Lines retain one native overhang pixel before their seed grid;
    // demo gutters are never mistaken for text advance or line spacing.
    let (origin, content_height) = if quad {
        (atlas.gutter_px, atlas.em_height_px)
    } else { (atlas.gutter_px.saturating_sub(1), atlas.em_height_px + 1) };
    let sx = origin + ((2 * x + 1) * content_width / (2 * width)).min(content_width - 1);
    let sy = origin + ((2 * y + 1) * content_height / (2 * height)).min(content_height - 1);
    mask[(sy.min(atlas.glyph_height - 1) * atlas.glyph_width + sx.min(atlas.glyph_width - 1)) as usize]
}

fn sample_bitmap(bits: u128, character: char, x: u32, y: u32, width: u32, height: u32) -> u8 {
    let sx = x as usize * microfont::FWIDTH / width as usize;
    let sy = y as usize * microfont::FHEIGHT / height as usize;
    let bias = usize::from(microfont::glyph_byte(character) == b'q');
    if sx < bias { return 0; }
    let bit = sy * microfont::FWIDTH + sx - bias;
    let count = microfont::FWIDTH * microfont::FHEIGHT;
    if bit < count && bits & (1 << (count - 1 - bit)) != 0 { 255 } else { 0 }
}

fn tint(coverage: u8, foreground: [u8; 4], background: [u8; 4]) -> [u8; 4] {
    let a = (u32::from(coverage) * u32::from(foreground[3]) + 127) / 255;
    let mut pixel = [0; 4];
    for channel in 0..3 {
        let bg = (u32::from(background[channel]) * u32::from(background[3]) + 127) / 255;
        pixel[channel] = ((u32::from(foreground[channel]) * a + bg * (255 - a) + 127) / 255) as u8;
    }
    pixel[3] = (a + (u32::from(background[3]) * (255 - a) + 127) / 255) as u8;
    pixel
}
