//! Publish a text page once; the animation task only reads this immutable page.
//! The normal Shell3 frame stays the pristine editor source. Effect output has
//! its own frame, so animation never changes text-cache pixels or their damage
//! baseline, and a new keystroke cannot capture an earlier animation frame.

use alloc::{sync::Arc, vec::Vec};
use trueos_terminal::edit_life::{CellRect, EditLife};

use crate::ui4::{
    acquire_published_frame, published_rgba_view, release_published_frame,
    edit_life::{Page, PixelRect},
};

use super::{RgbaColor, Ui4Surface};

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
    copy_source(surface, &mut page.rgba)?;
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
            let transparent = if clear_background {
                Some(crate::r::services::shell3_font_service::glyph(
                    cell.0, cell_width, cell_height, foreground, [0; 4], underline,
                )?)
            } else { None };
            let transparent_source = transparent.map(|copy| {
                let start = copy.source_y as usize * copy.source.pitch_bytes as usize
                    + copy.source_x as usize * 4;
                let source = crate::phys::phys_to_virt(copy.source.phys as usize) as *const u8;
                (unsafe { source.add(start) }, copy.source.pitch_bytes as usize)
            });
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
                    if let Some((source, source_pitch)) = transparent_source {
                        // glyph() owns immutable, already tinted source cells.
                        // Copy their exact premultiplied AA pixels; applying
                        // the coverage a second time would dim glyph edges.
                        let offset = y as usize * source_pitch + x as usize * 4;
                        let pixel = unsafe { core::slice::from_raw_parts(source.add(offset), 4) };
                        page.rgba[index * 4..index * 4 + 4].copy_from_slice(pixel);
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
    Ok(page)
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
