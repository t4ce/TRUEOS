//! Publish compact bitmap glyph cells once; animation reads immutable text input.
//! The normal Shell3 frame stays the pristine editor source. Effect output has
//! its own frame, so animation never changes text-cache pixels or their damage
//! baseline, and a new keystroke cannot capture an earlier animation frame.

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use trueos_terminal::edit_life::{CellRect, EditLife};

use crate::ui4::{
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
    if crate::ui4::edit_life::is_failed(surface.window)
        || !settings.active || !settings.exclusions.is_empty() {
        // Menus and non-page focus use the ordinary terminal painter. Keep the
        // last clean document input and world without capturing overlay text.
        crate::ui4::edit_life::pause(surface.window, surface.frame);
        surface.edit_life = Some(settings.clone());
        return;
    }
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
    let viewport = pixel_rect(settings.viewport, cell_width, cell_height, width, height);
    if viewport.width == 0 || viewport.height == 0 { return Err("shell3-life-empty-page"); }
    let columns = viewport.width.div_ceil(cell_width);
    let rows = viewport.height.div_ceil(cell_height);
    let count = (columns as usize).checked_mul(rows as usize).ok_or("shell3-life-page-size")?;
    let mut cells = Vec::new();
    cells.try_reserve_exact(count).map_err(|_| "shell3-life-allocation")?;
    for row in 0..rows {
        let line = lines.get(settings.viewport.y as usize + row as usize).copied().unwrap_or(&[]);
        for column in 0..columns {
            let cell = line.get(settings.viewport.x as usize + column as usize).copied()
                .unwrap_or((' ', None));
            let style = cell.1.unwrap_or(super::FOREGROUND);
            let (glyph, foreground) = crate::r::services::shell3_font_service::hex::resolve(cell.0, style.rgba());
            let background = style.background().unwrap_or(super::BACKGROUND);
            let flags = u32::from(background != [0, 0, 0, 255])
                | (u32::from(style.underline()) << 1);
            cells.push([glyph, u32::from_le_bytes(foreground), u32::from_le_bytes(background), flags]);
        }
    }
    let page = Page { width, height, viewport, cell_width, cell_height, columns, rows, cells,
        scroll_revision: settings.scroll_revision, emission_revision: settings.emission_revision };
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
            "edit-life: capture window={} size={}x{} first={} reset={} capture_ms={} glyph_cells={} input_bytes={} source_read_bytes=0\n",
            surface.window.raw(), width, height, first, reset, capture_ms, count, count * 16);
    }
    Ok(page)
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
