//! Shell3's shared native raster font authority. Styled RGBA cells are warmed
//! once and copied by either the CPU or legacy BLT. Shell3 owns layout,
//! clipping and sink choice; every output shares the native line/quad atlas.
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use super::microfont_mask_service::{OwnedAlphaAtlas, build_shell3_alpha_atlas};
use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaSurface, gpgpu::GpgpuOwnedRgba8Surface};

#[path = "shell3_hex_font.rs"]
pub(crate) mod hex;

const PAGE_COLUMNS: u32 = 8;
const PAGE_ROWS: u32 = 8;
pub(crate) const DEFAULT_UI4_FONT_SCALE: u32 = 2;
const ATLAS_EM_HALF_MM: u32 = 15;
const LAPTOP_SHELL_EM_HALF_MM: u32 = 8;

/// Scale 2 uses a 4-mm em on the physical Tiger Lake laptop and a 7.5-mm em
/// elsewhere. Unknown physical size retains the font's 11-row pixel fallback.
/// Frame, paint, scrolling and hit testing share this presentation size; the
/// one native atlas retains its canonical 7.5-mm production size.
pub(crate) fn cell_extent(scale: u32) -> Option<(u32, u32)> {
    let em_half_mm = if crate::pci::experimental_tgl_9a49_active() {
        LAPTOP_SHELL_EM_HALF_MM
    } else {
        ATLAS_EM_HALF_MM
    };
    cell_extent_for_em(scale, em_half_mm)
}

fn cell_extent_for_em(scale: u32, em_half_mm: u32) -> Option<(u32, u32)> {
    let numerator = em_half_mm.checked_mul(scale)?;
    let height = if let Some((_, height)) = crate::ui4::physical_extent_pixels_fraction(
        numerator, numerator, 2 * DEFAULT_UI4_FONT_SCALE) {
        height
    } else {
        crate::ui4::output_dimensions()?;
        (microfont::FHEIGHT as u32).checked_mul(scale)?
    };
    let width = height.checked_mul(microfont::FWIDTH as u32)?
        .checked_add(microfont::FHEIGHT as u32 / 2)? / microfont::FHEIGHT as u32;
    (width != 0 && height != 0).then_some((width, height))
}

/// Paper's grid text em is one 5-mm cell per scale step. Share its exact
/// physical conversion and rounded advance with other UI text consumers;
/// the glyph pixels still come from the one service-lifetime R8 atlas.
pub(crate) fn grid_cell_extent(scale: u32) -> Option<(u32, u32)> {
    if !(1..=8).contains(&scale) { return None; }
    let millimeters = 5 * scale;
    let height = crate::ui4::physical_extent_pixels(millimeters, millimeters)?.1;
    let width = height.checked_mul(6)?.checked_add(5)? / 11;
    (width != 0 && height != 0).then_some((width, height))
}
static FONT: spin::Once<Font> = spin::Once::new();
static PREPARING: AtomicBool = AtomicBool::new(false);
static NATIVE_BOOT_PREFLIGHT: spin::Once<Result<(), &'static str>> = spin::Once::new();
type Reply = Signal<crate::wait::EmbassySpinRawMutex, Result<(), &'static str>>;

struct Font {
    alpha: &'static OwnedAlphaAtlas,
    cache: spin::Mutex<CellCache>,
}
/// Edit's shader viewport selects Hex explicitly; layout remains shared.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum FontFace { Raster, Hex }

/// Compatibility query for callers that formerly exposed a laptop bitmap mode.
pub(crate) fn cpu_bitmap_fallback_enabled() -> bool {
    false
}

fn effective_face(_face: FontFace) -> FontFace {
    FontFace::Raster
}

/// Temporary ordering for the physical laptop bring-up: prove the native font
/// producer before Spirit can submit its first competing RCS workload. This
/// does not gate input, UI4 scanout, or Spirit's independent cursor task.
pub(crate) fn native_font_first_bringup_enabled() -> bool {
    crate::pci::experimental_tgl_9a49_active()
}

fn native_boot_preflight() -> Result<(), &'static str> {
    if !native_font_first_bringup_enabled() { return Ok(()); }
    // The warm task already waits for these conditions. Paper can also demand
    // a glyph, so do not turn early demand into a cached failed GPU attempt.
    if !crate::intel::guc_submission_ready() || crate::intel::render::warm_state().is_none() {
        return Err("render-busy");
    }
    *NATIVE_BOOT_PREFLIGHT.call_once(|| {
        crate::log_important!(target: "microfont-mask";
            "microfont: native-r8-preflight begin target=16x16 pixels=256 scope=font-first-laptop-bringup spirit_gpu=held fallback=none\n");
        match crate::intel::render::probe_native_microfont_r8() {
            Ok(proof) => {
                let passed = proof.passed();
                crate::log_important!(target: "microfont-mask";
                    "microfont: native-r8-preflight result pass={} draws={} release={} white_inside={}/64 black_outside={}/192 mismatched={} resources_released={} gpu_poll_us={} next={}\n",
                    passed as u8, proof.completed_draws, proof.release_sequence,
                    proof.pixels.white_inside, proof.pixels.black_outside,
                    proof.pixels.mismatched, proof.resources_released as u8,
                    proof.gpu_poll_us, if passed { "full-native-atlas" } else { "hold-spirit-gpu" });
                if passed { Ok(()) } else { Err("microfont-native-r8-preflight-failed") }
            }
            Err(reason) => {
                // An allocation/admission error is not evidence of bad shader
                // output. Retain the precise cause and never retry uncertain
                // GPU work just to obtain a more convenient boot result.
                crate::log_important!(target: "microfont-mask";
                    "microfont: native-r8-preflight error={} pass=0 action=hold-spirit-gpu attempts=1 fallback=none\n",
                    reason);
                Err("microfont-native-r8-preflight-failed")
            }
        }
    })
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct CellKey {
    face: FontFace,
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
/// the same initialization; font production runs on a blocking service lane.
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
            Err("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy" | "virgl-offscreen-busy") => {
                Timer::after(Duration::from_millis(20)).await;
            }
            result => return result,
        }
    }
}

fn prepare_once() -> Result<(), &'static str> {
    if FONT.get().is_some() { return Ok(()); }
    let (_, height) = cell_extent_for_em(DEFAULT_UI4_FONT_SCALE, ATLAS_EM_HALF_MM)
        .ok_or("shell3-font-physical-size-unavailable")?;
    native_boot_preflight()?;
    let alpha = build_shell3_alpha_atlas(height)?;
    alpha.glyph('?').ok_or("shell3-font-missing-question-mark")?;
    let glyphs = alpha.glyphs().count();
    FONT.call_once(|| Font { alpha, cache: spin::Mutex::new(CellCache::default()) });
    crate::log_important!(target: "microfont-mask";
        "shell3-font: ready source=native-line-quad producer=native-gpu format=r8 em_mm=7.5 em_height_px={} glyphs={} successful_productions=1 retained=service-lifetime layout=physical-cells missing_glyph=red-question-mark\n",
        height, glyphs);
    if crate::pci::experimental_tgl_9a49_active() {
        if let Some((width, presentation_height)) = cell_extent(DEFAULT_UI4_FONT_SCALE) {
            crate::log_important!(target: "microfont-mask";
                "shell3-font: presentation em_mm=4 cell={}x{} scale=2 atlas_em_height_px={} sampling=shared-native-atlas\n",
                width, presentation_height, height);
        }
    }
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
    glyph_with_face(FontFace::Raster, character, cell_width, cell_height, foreground, background, underline)
}

pub(crate) fn glyph_with_face(
    face: FontFace,
    character: char,
    cell_width: u32,
    cell_height: u32,
    foreground: [u8; 4],
    background: [u8; 4],
    underline: bool,
) -> Result<GucBcs0RgbaCopy, &'static str> {
    let face = effective_face(face);
    let font = FONT.get().ok_or("shell3-font-not-ready")?;
    if cell_width == 0 || cell_height == 0 || cell_width > 256 || cell_height > 256 {
        return Err("shell3-font-cell-size");
    }
    let key = CellKey { face, character, width: cell_width, height: cell_height, foreground, background, underline };
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
    let (mask, foreground) = resolved_face(font, face, character, foreground)?;
    let mut first_pixel = None;
    let mut uniform = true;
    for y in 0..cell_height {
        for x in 0..cell_width {
            let coverage = sample_face(font, mask, x, y, cell_width, cell_height, underline);
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

/// Visit the exact retained coverage used by `glyph`, including underlines and
/// the missing-character fallback. This only samples the existing atlas; it
/// never rasterizes text or changes the shared font tiles.
pub(crate) fn with_cell_coverage(
    character: char,
    cell_width: u32,
    cell_height: u32,
    foreground: [u8; 4],
    underline: bool,
    pixel: impl FnMut(u32, u32, u8, [u8; 4]),
) -> Result<(), &'static str> {
    with_cell_coverage_with_face(FontFace::Raster, character, cell_width, cell_height, foreground, underline, pixel)
}

/// An explicit native stroke variant; dimensions and character advance stay
/// identical to the requested texture. Cold variants schedule a blocking job.
pub(crate) fn with_styled_cell_coverage(
    character: char, cell_width: u32, cell_height: u32,
    foreground: [u8; 4], extra_stroke_px: f32,
    mut pixel: impl FnMut(u32, u32, u8, [u8; 4]),
) -> Result<(), &'static str> {
    if extra_stroke_px == 0.0 {
        return with_cell_coverage(character, cell_width, cell_height, foreground, false, pixel);
    }
    if cell_width != (cell_height * 6 + 5) / 11 { return Err("microfont-style-shape"); }
    let mask = super::microfont_mask_service::style::glyph(character, cell_height, extra_stroke_px)?;
    for y in 0..cell_height {
        for x in 0..cell_width {
            pixel(x, y, mask[(y * cell_width + x) as usize], foreground);
        }
    }
    Ok(())
}

pub(crate) fn with_cell_coverage_with_face(
    face: FontFace,
    character: char,
    cell_width: u32,
    cell_height: u32,
    foreground: [u8; 4],
    underline: bool,
    mut pixel: impl FnMut(u32, u32, u8, [u8; 4]),
) -> Result<(), &'static str> {
    let font = FONT.get().ok_or("shell3-font-not-ready")?;
    if cell_width == 0 || cell_height == 0 || cell_width > 256 || cell_height > 256 {
        return Err("shell3-font-cell-size");
    }
    let (mask, foreground) = resolved_face(font, face, character, foreground)?;
    for y in 0..cell_height {
        for x in 0..cell_width {
            pixel(x, y, sample_face(font, mask, x, y, cell_width, cell_height, underline), foreground);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum CellMask<'a> { Raster(&'a [u8]), TabLabel(hex::tab_label_cell::TabLabelCell) }

fn resolved_face(font: &Font, _face: FontFace, character: char, foreground: [u8; 4])
    -> Result<(CellMask<'_>, [u8; 4]), &'static str> {
    if let Some(label) = hex::tab_label_cell::TabLabelCell::from_character(character) {
        return Ok((CellMask::TabLabel(label), foreground));
    }
    resolved_glyph(font, character, foreground)
        .map(|(mask, foreground)| (CellMask::Raster(mask), foreground))
}

fn sample_face(font: &Font, mask: CellMask<'_>, x: u32, y: u32,
    width: u32, height: u32, underline: bool) -> u8 {
    match mask {
        CellMask::Raster(mask) => sample_cell(font.alpha, mask, x, y, width, height, underline),
        CellMask::TabLabel(label) => {
            let Some((character, px, py, w, h)) = label.point(x, y, width, height) else { return 0; };
            font.alpha.glyph(character).map_or(0, |mask| sample_native(font.alpha, mask, px, py, w, h))
        }
    }
}

fn resolved_glyph(font: &Font, character: char, foreground: [u8; 4])
    -> Result<(&[u8], [u8; 4]), &'static str> {
    match font.alpha.glyph(character) {
        Some(mask) => Ok((mask, foreground)),
        None => Ok((font.alpha.glyph('?').ok_or("shell3-font-missing-question-mark")?,
            [255, 0, 0, foreground[3]])),
    }
}

fn sample_cell(atlas: &OwnedAlphaAtlas, mask: &[u8], x: u32, y: u32,
    width: u32, height: u32, underline: bool) -> u8 {
    if underline && y * microfont::FHEIGHT as u32 / height == microfont::FHEIGHT as u32 - 1 {
        255
    } else {
        sample_native(atlas, mask, x, y, width, height)
    }
}

fn sample_native(atlas: &OwnedAlphaAtlas, mask: &[u8],
    x: u32, y: u32, width: u32, height: u32) -> u8 {
    atlas.sample(mask, x, y, width, height)
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

// Gridpape scales the shared R8 tiles without producing additional alphabets.
pub(crate) async fn grid_glyph(scale: u32, ch: char) -> Result<Vec<u8>, &'static str> {
    if !(1..=8).contains(&scale) { return Err("grid-font-scale"); }
    prepare().await?;
    type GlyphReply = Signal<crate::wait::EmbassySpinRawMutex, Result<Vec<u8>, &'static str>>;
    loop {
        let reply = alloc::sync::Arc::new(GlyphReply::new());
        let response = alloc::sync::Arc::clone(&reply);
        let job = Box::new(move || response.signal(grid_glyph_blocking(scale, ch)));
        if let Err(job) = crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "grid-font") {
            drop(job);
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        match reply.wait().await {
            Err("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy" | "virgl-offscreen-busy") =>
                Timer::after(Duration::from_millis(20)).await,
            result => return result,
        }
    }
}

fn grid_glyph_blocking(scale: u32, ch: char) -> Result<Vec<u8>, &'static str> {
    let font = FONT.get().ok_or("shell3-font-not-ready")?;
    let (width, height) = grid_cell_extent(scale).ok_or("grid-font-physical-size")?;
    let mut result = Vec::with_capacity(8 + (width * height * 4) as usize);
    result.extend_from_slice(&width.to_le_bytes());
    result.extend_from_slice(&height.to_le_bytes());
    let atlas = font.alpha;
    let mask = atlas.glyph(ch).or_else(|| atlas.glyph('?')).ok_or("grid-font-glyph")?;
    for y in 0..height {
        for x in 0..width {
            let alpha = sample_native(atlas, mask, x, y, width, height);
            let value = 255 - alpha;
            result.extend_from_slice(&[value, value, value, 255]);
        }
    }
    Ok(result)
}

/// Publish text once the font atlas is ready.
#[cfg(not(test))]
#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_warm_task() {
    loop {
        match prepare().await {
            Ok(()) => {
                super::microfont_log_service::font_ready();
                crate::ui4::microfont_ready();
                break;
            }
            Err(reason) => {
                crate::log_warn!(target: "microfont-mask";
                    "microfont: warm deferred reason={}\n", reason);
                trueos_time::Timer::after(trueos_time::Duration::from_millis(100)).await;
            }
        }
    }
}
