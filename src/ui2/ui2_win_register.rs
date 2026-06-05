#![allow(dead_code)]

use super::*;

// ---------------------------------------------------------------------------
// Minimized-window strip (left-aligned, top-to-bottom).
// ---------------------------------------------------------------------------

pub(super) fn minimized_window_strip_rect(state: &Ui2State, window_id: u32) -> Option<Ui2Rect> {
    let mut slot = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window.visible || window.state != Ui2WindowStateKind::Minimized {
            continue;
        }
        if window.id == window_id {
            let x = UI2_MINIMIZED_STRIP_PAD;
            let y =
                UI2_MINIMIZED_STRIP_PAD + (slot as f32 * (UI2_TITLE_H + UI2_MINIMIZED_STRIP_GAP));
            let max_w = ((state.view_w as f32) - x - UI2_MINIMIZED_STRIP_PAD).max(96.0);
            return Some(Ui2Rect::new(x, y, UI2_MINIMIZED_STRIP_W.min(max_w), UI2_TITLE_H));
        }
        slot = slot.saturating_add(1);
    }
    None
}

// ---------------------------------------------------------------------------
// Offline-task dock (right-aligned pills for disabled / X-closed windows).
// ---------------------------------------------------------------------------

const UI2_OFFLINE_PILL_W: f32 = 200.0;
const UI2_OFFLINE_PILL_H: f32 = UI2_BAR_H;
const UI2_OFFLINE_PILL_GAP: f32 = 4.0;
const UI2_OFFLINE_PILL_PAD: f32 = 8.0;
const UI2_OFFLINE_PILL_PLAY_SIZE: f32 = UI2_BAR_H;
const UI2_OFFLINE_PILL_BG: (u8, u8, u8, u8) = (0xD9, 0xDE, 0xE5, 0xD0);
const UI2_OFFLINE_PILL_TEXT: (u8, u8, u8, u8) = (0x30, 0x30, 0x30, 0xFF);
/// Play button: ▶ U+25B6
const UI2_OFFLINE_PLAY_TWEMOJI: char = '\u{23EF}';

/// One entry in the offline dock.
#[derive(Copy, Clone)]
struct OfflinePillSlot {
    rect: Ui2Rect,
    /// If > 0, this pill can re-show an existing hidden window.
    window_id: u32,
    /// If < usize::MAX, this pill enables a disabled task.
    task_index: usize,
}

/// Offline pills rebuilt every frame.
static OFFLINE_PILLS: Mutex<Vec<OfflinePillSlot>> = Mutex::new(Vec::new());

/// Draw the offline-task dock on the right side and populate OFFLINE_PILLS for hit-testing.
pub(super) fn draw_offline_dock(state: &Ui2State) {
    if crate::gfx::is_intel_active() {
        *OFFLINE_PILLS.lock() = Vec::new();
        return;
    }

    let mut pills: Vec<OfflinePillSlot> = Vec::new();

    // 1. Disabled tasks not yet started.
    for entry in crate::r::spawn_service::offline_ui2_demo_tasks() {
        let window_id = state
            .windows
            .iter()
            .find(|window| {
                window.spawn_task_index == Some(entry.index)
                    && crate::r::spawn_service::task_started_by_index(entry.index)
                    && !window.visible
                    && window.kind == Ui2WindowKind::HostedSurface
            })
            .map(|window| window.id)
            .unwrap_or(0);
        pills.push(OfflinePillSlot {
            rect: Ui2Rect::default(),
            window_id,
            task_index: entry.index,
        });
    }

    // 2. Hidden (X-closed) windows.
    for window in &state.windows {
        if window.visible || window.title.is_empty() {
            continue;
        }
        if window.spawn_task_index.is_some() {
            continue;
        }
        // Only show hosted-surface windows (demo apps) that were X-closed.
        if window.kind != Ui2WindowKind::HostedSurface {
            continue;
        }
        pills.push(OfflinePillSlot {
            rect: Ui2Rect::default(),
            window_id: window.id,
            task_index: usize::MAX,
        });
    }

    if pills.is_empty() {
        *OFFLINE_PILLS.lock() = Vec::new();
        return;
    }

    // Layout: stack top-to-bottom, right-aligned.
    let pill_w = UI2_OFFLINE_PILL_W.min((state.view_w as f32) - UI2_OFFLINE_PILL_PAD * 2.0);
    let base_x = (state.view_w as f32) - pill_w - UI2_OFFLINE_PILL_PAD;
    let mut y = UI2_OFFLINE_PILL_PAD;

    for pill in pills.iter_mut() {
        pill.rect = Ui2Rect::new(base_x, y, pill_w, UI2_OFFLINE_PILL_H);
        y += UI2_OFFLINE_PILL_H + UI2_OFFLINE_PILL_GAP;
    }

    // Draw.
    for pill in pills.iter() {
        // Background pill.
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            pill.rect.x,
            pill.rect.y,
            pill.rect.w,
            pill.rect.h,
            UI2_OFFLINE_PILL_BG,
            state.view_w,
            state.view_h,
        );

        // Title text (left side of pill).
        let title: &str = if pill.window_id != 0 {
            state
                .windows
                .iter()
                .find(|w| w.id == pill.window_id)
                .map(|w| w.title.as_str())
                .unwrap_or("?")
        } else if pill.task_index < usize::MAX {
            crate::r::spawn_service::task_name_by_index(pill.task_index).unwrap_or("?")
        } else {
            "?"
        };
        let title_rect = Ui2Rect::new(
            pill.rect.x + 4.0,
            pill.rect.y,
            (pill.rect.w - UI2_OFFLINE_PILL_PLAY_SIZE - 8.0).max(1.0),
            pill.rect.h,
        );
        let title_display = pill_title_with_ellipsis(title, title_rect.w);
        let _ = ui2_font_draw_text_line_in_rect_with_tier_rgba_no_present(
            title_display.as_str(),
            title_rect,
            UI2_OFFLINE_TITLE_FONT_TIER,
            Ui2FontTextAlign::Left,
            Ui2FontVerticalAlign::Center,
            state.view_w,
            state.view_h,
            UI2_OFFLINE_PILL_TEXT,
        );

        // Play button (right side of pill).
        let play_rect = Ui2Rect::new(
            pill.rect.x + pill.rect.w - UI2_OFFLINE_PILL_PLAY_SIZE,
            pill.rect.y,
            UI2_OFFLINE_PILL_PLAY_SIZE,
            UI2_OFFLINE_PILL_PLAY_SIZE,
        );
        draw_window_twemoji_button_standalone(state, play_rect, UI2_OFFLINE_PLAY_TWEMOJI, 0xFF);
    }

    *OFFLINE_PILLS.lock() = pills;
}

const UI2_OFFLINE_TITLE_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;

fn pill_title_with_ellipsis(text: &str, max_width_px: f32) -> alloc::string::String {
    if text.is_empty() || max_width_px <= 0.0 {
        return alloc::string::String::new();
    }
    if (ui2_font_measure_text(UI2_OFFLINE_TITLE_FONT_TIER, text).width_px as f32) <= max_width_px {
        return alloc::string::String::from(text);
    }
    const ELLIPSIS: &str = "...";
    let ellipsis_w = ui2_font_measure_text(UI2_OFFLINE_TITLE_FONT_TIER, ELLIPSIS).width_px as f32;
    if ellipsis_w > max_width_px {
        return alloc::string::String::new();
    }
    let mut out = alloc::string::String::new();
    let mut used_w = 0.0f32;
    for ch in text.chars() {
        let ch_w = f32::from(ui2_font_char_advance_px(UI2_OFFLINE_TITLE_FONT_TIER, ch).max(1));
        if used_w + ch_w + ellipsis_w > max_width_px {
            break;
        }
        out.push(ch);
        used_w += ch_w;
    }
    out.push_str(ELLIPSIS);
    out
}

/// Draw a twemoji icon in a rect (standalone, not attached to a window).
fn draw_window_twemoji_button_standalone(
    state: &Ui2State,
    rect: Ui2Rect,
    ch: char,
    alpha: u8,
) -> bool {
    let Some(glyph) = ui2_font_resolve_glyph(Ui2FontTier::OneX, ch) else {
        return false;
    };
    if !glyph.ready {
        return false;
    }
    let Some(texture) = glyph.texture else {
        return false;
    };

    let inset_rect =
        Ui2Rect::new(rect.x + 1.0, rect.y + 1.0, (rect.w - 2.0).max(1.0), (rect.h - 2.0).max(1.0));
    let crop_px = 1.0f32;
    let src_x = f32::from(glyph.region.src_x) + crop_px;
    let src_y = f32::from(glyph.region.src_y) + crop_px;
    let src_w = f32::from(glyph.region.src_w.max(3)) - (crop_px * 2.0);
    let src_h = f32::from(glyph.region.src_h.max(3)) - (crop_px * 2.0);
    let scale = libm::fminf(inset_rect.w / src_w, inset_rect.h / src_h);
    let draw_w = libm::fmaxf(1.0, src_w * scale);
    let draw_h = libm::fmaxf(1.0, src_h * scale);
    let draw_x = inset_rect.x + ((inset_rect.w - draw_w) * 0.5).max(0.0);
    let draw_y = inset_rect.y + ((inset_rect.h - draw_h) * 0.5).max(0.0);
    let atlas_w = f32::from(glyph.region.atlas_w.max(1));
    let atlas_h = f32::from(glyph.region.atlas_h.max(1));

    draw_texture_rect_uv_rgba_no_present(
        texture.tex_id,
        draw_x,
        draw_y,
        draw_w,
        draw_h,
        src_x / atlas_w,
        src_y / atlas_h,
        (src_x + src_w) / atlas_w,
        (src_y + src_h) / atlas_h,
        state.view_w,
        state.view_h,
        true,
        (255, 255, 255, alpha),
    )
}

/// Handle a click at (x,y) on the offline dock. Returns true if consumed.
pub(super) fn handle_offline_dock_click(state: &mut Ui2State, x: f32, y: f32) -> bool {
    let pills = OFFLINE_PILLS.lock().clone();
    for pill in pills.iter() {
        if !rect_contains_point(pill.rect, x, y) {
            continue;
        }
        if pill.window_id != 0 {
            // Re-show a hidden window.
            if pill.task_index != usize::MAX {
                crate::r::spawn_service::enable_task_by_index(pill.task_index);
            }
            set_window_visible_in_state(state, pill.window_id, true);
            restore_window_in_state(state, pill.window_id);
            return true;
        }
        if pill.task_index != usize::MAX {
            // Enable the disabled task — spawn service will pick it up.
            crate::r::spawn_service::enable_task_by_index(pill.task_index);
            return true;
        }
    }
    false
}
