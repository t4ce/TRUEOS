//! Publish a text page once; the animation task only reads this immutable page.
//! The normal Shell3 frame stays the pristine editor source. Effect output has
//! its own frame, so animation never changes text-cache pixels or their damage
//! baseline, and a new keystroke cannot capture an earlier animation frame.

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use trueos_terminal::edit_life::{CellRect, EditLife};

use crate::ui4::{
    acquire_published_frame, published_rgba_view, release_published_frame,
    edit_life::{Page, PixelRect},
};

use super::{RgbaColor, Ui4Surface};

static LAST_CAPTURE_LOG_MS: AtomicU64 = AtomicU64::new(0);

pub(super) fn present(
    surface: &mut Ui4Surface,
    lines: &[&[(char, Option<RgbaColor>)]],
    settings: Option<&EditLife>,
    source_changed: bool,
) {
    let Some(settings) = settings.filter(|settings| {
        settings.enabled && settings.viewport.width != 0 && settings.viewport.height != 0
    }) else {
        if surface.edit_life.take().is_some() {
            crate::ui4::edit_life::remove(surface.window);
            surface.edit_life_page = None;
        }
        return;
    };
    if !source_changed && surface.edit_life.as_ref() == Some(settings) {
        return;
    }
    let page = match capture(surface, lines, settings) {
        Ok(page) => Arc::new(page),
        Err(error) => {
            crate::ui4::edit_life::remove(surface.window);
            surface.edit_life = None;
            surface.edit_life_page = None;
            crate::log_warn!(target: "ui4/edit-life";
                "edit-life: capture skipped window={} reason={} source=opaque-editor\n",
                surface.window.raw(), error);
            return;
        }
    };
    crate::ui4::edit_life::publish_page(surface.window, surface.frame, Arc::clone(&page));
    surface.edit_life = Some(settings.clone());
    surface.edit_life_page = Some(page);
}

fn capture(
    surface: &Ui4Surface,
    lines: &[&[(char, Option<RgbaColor>)]],
    settings: &EditLife,
) -> Result<Page, &'static str> {
    let started_ms = trueos_time::Instant::now().as_millis();
    let (cell_width, cell_height) =
        crate::r::services::shell3_font_service::cell_extent(surface.scale)
            .ok_or("shell3-life-font-size")?;
    let width = surface.width;
    let height = surface.height;
    let pixels = (width as usize).checked_mul(height as usize)
        .ok_or("shell3-life-page-size")?;
    if pixels == 0 || pixels > crate::ui4::edit_life::MAX_PAGE_PIXELS {
        return Err("shell3-life-page-limit");
    }
    let bytes = pixels.checked_mul(4).ok_or("shell3-life-page-size")?;
    let viewport = pixel_rect(settings.viewport, cell_width, cell_height, width, height);
    let mut page = Page {
        width, height,
        rgba: zeroed(bytes)?,
        mask: zeroed(pixels)?,
        colors: zeroed(bytes)?,
        viewport,
        scroll_revision: settings.scroll_revision,
        emission_revision: settings.emission_revision,
    };
    let source_started_ms = trueos_time::Instant::now().as_millis();
    copy_source(surface, &mut page.rgba)?;
    let source_read_ms = trueos_time::Instant::now().as_millis().saturating_sub(source_started_ms);
    let mut exclusions = Vec::new();
    exclusions.try_reserve_exact(settings.exclusions.len()).map_err(|_| "shell3-life-allocation")?;
    exclusions.extend(settings.exclusions.iter().copied()
        .map(|rect| pixel_rect(rect, cell_width, cell_height, width, height)));

    for (row, line) in lines.iter().enumerate() {
        let top = (row as u32).saturating_mul(cell_height);
        if top >= viewport.y.saturating_add(viewport.height) { break; }
        if top.saturating_add(cell_height) <= viewport.y { continue; }
        for (column, cell) in line.iter().copied().enumerate() {
            let left = (column as u32).saturating_mul(cell_width);
            if left >= viewport.x.saturating_add(viewport.width) { break; }
            if left.saturating_add(cell_width) <= viewport.x { continue; }
            let style = cell.1.unwrap_or(super::FOREGROUND);
            let foreground = style.rgba();
            let background = style.background().unwrap_or(super::BACKGROUND);
            let underline = style.underline();
            // Edit's normal page is black. Retain opaque cursor-line,
            // selection and inverted block-cursor backgrounds as painted by
            // Edit; the shader never changes their foreground or background.
            let clear_background = background == [0, 0, 0, 255];
            if cell.0 == ' ' && !underline {
                // Blank cells have no input ink. Their mask/color allocations
                // are already zero, so avoid even visiting the retained atlas.
                if clear_background {
                    clear_cell(&mut page.rgba, width, viewport, &exclusions,
                        left, top, cell_width, cell_height);
                }
                continue;
            }
            crate::r::services::shell3_font_service::with_cell_coverage(
                cell.0, cell_width, cell_height, foreground, underline,
                |x, y, coverage, resolved_foreground| {
                    let px = left.saturating_add(x);
                    let py = top.saturating_add(y);
                    if !contains(viewport, px, py) { return; }
                    let index = py as usize * width as usize + px as usize;
                    page.mask[index] = coverage;
                    page.colors[index * 4..index * 4 + 4]
                        .copy_from_slice(&[resolved_foreground[0], resolved_foreground[1], resolved_foreground[2], coverage]);
                    if exclusions.iter().any(|&rect| contains(rect, px, py)) { return; }
                    if clear_background {
                        // Match the font producer's tint over transparent
                        // black exactly, using its existing coverage once.
                        // No additional styled glyph/cache allocation is needed.
                        let alpha = (u32::from(coverage) * u32::from(resolved_foreground[3]) + 127) / 255;
                        let pixel = &mut page.rgba[index * 4..index * 4 + 4];
                        for channel in 0..3 {
                            pixel[channel] = ((u32::from(resolved_foreground[channel]) * alpha + 127) / 255) as u8;
                        }
                        pixel[3] = alpha as u8;
                    }
                },
            )?;
        }
    }

    // Menus and dialogs overwrite terminal cells. Keep the last document mask
    // underneath them rather than mistaking menu glyphs for newly typed text.
    if let Some(previous) = surface.edit_life_page.as_deref().filter(|previous| {
        previous.width == width && previous.height == height
            && previous.scroll_revision == settings.scroll_revision
            && previous.viewport.x == viewport.x && previous.viewport.y == viewport.y
            && previous.viewport.width == viewport.width
            && previous.viewport.height == viewport.height
    }) {
        for rect in exclusions {
            for y in rect.y..rect.y.saturating_add(rect.height) {
                for x in rect.x..rect.x.saturating_add(rect.width) {
                    if !contains(viewport, x, y) { continue; }
                    let index = y as usize * width as usize + x as usize;
                    page.mask[index] = previous.mask[index];
                    page.colors[index * 4..index * 4 + 4]
                        .copy_from_slice(&previous.colors[index * 4..index * 4 + 4]);
                }
            }
        }
    } else {
        // With no previous document page, overlays have no document coverage.
        for rect in exclusions {
            for y in rect.y..rect.y.saturating_add(rect.height) {
                for x in rect.x..rect.x.saturating_add(rect.width) {
                    let index = y as usize * width as usize + x as usize;
                    page.mask[index] = 0;
                    page.colors[index * 4..index * 4 + 4].fill(0);
                }
            }
        }
    }
    let finished_ms = trueos_time::Instant::now().as_millis();
    let capture_ms = finished_ms.saturating_sub(started_ms);
    let first = surface.edit_life_page.is_none();
    let reset = surface.edit_life_page.as_deref().is_some_and(|previous| {
        previous.scroll_revision != settings.scroll_revision || previous.viewport != viewport
    });
    let last_log_ms = LAST_CAPTURE_LOG_MS.load(Ordering::Relaxed);
    if first || ((reset || capture_ms >= 25)
        && finished_ms.saturating_sub(last_log_ms) >= 1000
        && LAST_CAPTURE_LOG_MS.compare_exchange(last_log_ms, finished_ms,
            Ordering::Relaxed, Ordering::Relaxed).is_ok())
    {
        LAST_CAPTURE_LOG_MS.store(finished_ms, Ordering::Relaxed);
        crate::log_info!(target: "ui4/edit-life";
            "edit-life: capture window={} size={}x{} first={} reset={} capture_ms={} source_read_ms={}\n",
            surface.window.raw(), width, height, first, reset, capture_ms, source_read_ms);
    }
    Ok(page)
}

/// Clear blank editor cells in row spans while leaving opaque overlays intact.
fn clear_cell(rgba: &mut [u8], width: u32, viewport: PixelRect, exclusions: &[PixelRect],
    left: u32, top: u32, cell_width: u32, cell_height: u32) {
    let right = left.saturating_add(cell_width).min(viewport.x + viewport.width);
    let bottom = top.saturating_add(cell_height).min(viewport.y + viewport.height);
    let left = left.max(viewport.x);
    for y in top.max(viewport.y)..bottom {
        let mut start = left;
        while start < right {
            if let Some(rect) = exclusions.iter().find(|&&rect| contains(rect, start, y)) {
                start = rect.x.saturating_add(rect.width).min(right);
                continue;
            }
            let end = exclusions.iter().filter(|rect| y >= rect.y && y - rect.y < rect.height
                && rect.x > start).map(|rect| rect.x).min().unwrap_or(right).min(right);
            let row = y as usize * width as usize;
            rgba[(row + start as usize) * 4..(row + end as usize) * 4].fill(0);
            start = end;
        }
    }
}

fn zeroed(bytes: usize) -> Result<Vec<u8>, &'static str> {
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(bytes).map_err(|_| "shell3-life-allocation")?;
    pixels.resize(bytes, 0);
    Ok(pixels)
}

fn copy_source(surface: &Ui4Surface, destination: &mut [u8]) -> Result<(), &'static str> {
    let lease = acquire_published_frame(surface.frame).map_err(|_| "shell3-life-source-busy")?;
    let result = published_rgba_view(lease).map_err(|_| "shell3-life-source-view")
        .map(|view| {
            // The normal painter has retired its copies before publication.
            // Keep this exact front pinned through the synchronous CPU read.
            let row_bytes = surface.width as usize * 4;
            for row in 0..surface.height as usize {
                let source = unsafe {
                    core::slice::from_raw_parts(view.virt.add(row * view.pitch as usize), row_bytes)
                };
                destination[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(source);
            }
        });
    let _ = release_published_frame(lease);
    result
}

fn pixel_rect(rect: CellRect, cell_width: u32, cell_height: u32,
    width: u32, height: u32) -> PixelRect {
    let x = rect.x.saturating_mul(cell_width).min(width);
    let y = rect.y.saturating_mul(cell_height).min(height);
    PixelRect {
        x, y,
        width: rect.width.saturating_mul(cell_width).min(width - x),
        height: rect.height.saturating_mul(cell_height).min(height - y),
    }
}

fn contains(rect: PixelRect, x: u32, y: u32) -> bool {
    x >= rect.x && y >= rect.y
        && x - rect.x < rect.width && y - rect.y < rect.height
}
