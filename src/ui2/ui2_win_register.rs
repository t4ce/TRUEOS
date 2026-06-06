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
const UI2_OFFLINE_PILL_BG: (u8, u8, u8, u8) = (0x52, 0xD2, 0x7A, 0xD8);
const UI2_OFFLINE_PILL_LINE: (u8, u8, u8, u8) = (0x52, 0xD2, 0x7A, 0xFF);
const UI2_OFFLINE_PILL_GRAD_LEFT: (u8, u8, u8, u8) = (0xC8, 0xCC, 0xC8, 0xFF);
const UI2_OFFLINE_PILL_GRAD_RIGHT: (u8, u8, u8, u8) = (0xF2, 0xF4, 0xF2, 0xFF);
const UI2_OFFLINE_PILL_TEXT: (u8, u8, u8, u8) = (0x30, 0x30, 0x30, 0xFF);
/// Play button: ▶ U+25B6
const UI2_OFFLINE_PLAY_TWEMOJI: char = '\u{23EF}';
const UI2_OFFLINE_APP_FALLBACK_TWEMOJI: char = '\u{25FC}';
const UI2_OFFLINE_APP_FONT_TWEMOJI: char = '\u{1F524}';
const UI2_OFFLINE_APP_CANVAS_TWEMOJI: char = '\u{1F4A0}';
const UI2_OFFLINE_APP_PLAYER_TWEMOJI: char = '\u{24C2}';
const UI2_OFFLINE_APP_RAPLE_TWEMOJI: char = '\u{26A1}';
const UI2_OFFLINE_APP_SHELL_TWEMOJI: char = '\u{1F40C}';
const UI2_OFFLINE_APP_SWARM_TWEMOJI: char = '\u{1F47D}';
const UI2_OFFLINE_APP_SMILEY_TWEMOJI: char = '\u{1F600}';
const UI2_OFFLINE_APP_PLAYABLE_TWEMOJI: char = '\u{23F5}';

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

fn collect_offline_pill_slots(state: &Ui2State) -> Vec<OfflinePillSlot> {
    let mut pills: Vec<OfflinePillSlot> = Vec::new();

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

    for window in &state.windows {
        if window.visible || window.title.is_empty() {
            continue;
        }
        if window.spawn_task_index.is_some() {
            continue;
        }
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
        return pills;
    }

    let pill_w = UI2_OFFLINE_PILL_W.min((state.view_w as f32) - UI2_OFFLINE_PILL_PAD * 2.0);
    let base_x = (state.view_w as f32) - pill_w - UI2_OFFLINE_PILL_PAD;
    let mut y = UI2_OFFLINE_PILL_PAD;

    for pill in pills.iter_mut() {
        pill.rect = Ui2Rect::new(base_x, y, pill_w, UI2_OFFLINE_PILL_H);
        y += UI2_OFFLINE_PILL_H + UI2_OFFLINE_PILL_GAP;
    }

    pills
}

fn sync_offline_pill_hit_slots(pills: &[OfflinePillSlot]) {
    *OFFLINE_PILLS.lock() = pills.to_vec();
}

fn offline_pill_play_rect(pill: &OfflinePillSlot) -> Ui2Rect {
    Ui2Rect::new(
        pill.rect.x + pill.rect.w - UI2_OFFLINE_PILL_PLAY_SIZE,
        pill.rect.y,
        UI2_OFFLINE_PILL_PLAY_SIZE,
        UI2_OFFLINE_PILL_PLAY_SIZE,
    )
}

pub(super) fn offline_dock_play_button_hover_rect_at(x: f32, y: f32) -> Option<Ui2Rect> {
    let pills = OFFLINE_PILLS.lock().clone();
    for pill in pills.iter() {
        let play_rect = offline_pill_play_rect(pill);
        if rect_contains_point(play_rect, x, y) {
            return Some(play_rect);
        }
    }
    None
}

fn offline_pill_title<'a>(state: &'a Ui2State, pill: &OfflinePillSlot) -> &'a str {
    if pill.window_id != 0 {
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
    }
}

fn offline_task_icon_char(task_name: &str) -> char {
    if task_name.contains("athlas")
        || task_name.contains("palatino")
        || task_name.contains("twemoji")
        || task_name.contains("text-input")
    {
        UI2_OFFLINE_APP_FONT_TWEMOJI
    } else if task_name.contains("coreticks")
        || task_name.contains("canvas3d")
        || task_name.contains("mandelbrot")
        || task_name.contains("render-album")
    {
        UI2_OFFLINE_APP_CANVAS_TWEMOJI
    } else if task_name.contains("player") {
        UI2_OFFLINE_APP_PLAYER_TWEMOJI
    } else if task_name.contains("raple") {
        UI2_OFFLINE_APP_RAPLE_TWEMOJI
    } else if task_name.contains("shell") {
        UI2_OFFLINE_APP_SHELL_TWEMOJI
    } else if task_name.contains("swarm") {
        UI2_OFFLINE_APP_SWARM_TWEMOJI
    } else if task_name.contains("smiley") {
        UI2_OFFLINE_APP_SMILEY_TWEMOJI
    } else if task_name.contains("gboi") {
        UI2_OFFLINE_APP_PLAYABLE_TWEMOJI
    } else {
        UI2_OFFLINE_APP_FALLBACK_TWEMOJI
    }
}

fn offline_pill_icon_char(state: &Ui2State, pill: &OfflinePillSlot) -> char {
    if pill.window_id != 0
        && let Some(ch) = state
            .windows
            .iter()
            .find(|w| w.id == pill.window_id)
            .filter(|w| w.title_icon_visible && w.title_twemoji != '\0')
            .map(|w| w.title_twemoji)
    {
        return ch;
    }
    if pill.task_index < usize::MAX
        && let Some(task_name) = crate::r::spawn_service::task_name_by_index(pill.task_index)
    {
        return offline_task_icon_char(task_name);
    }
    UI2_OFFLINE_APP_FALLBACK_TWEMOJI
}

fn push_offline_twemoji_sprite64_placement(
    out: &mut Vec<crate::intel::gpgpu::GpgpuTwemojiSprite64Placement>,
    ch: char,
    rect: Ui2Rect,
) -> bool {
    if super::ui2_win_deco::push_chrome_twemoji_sprite64_placement(out, ch, rect) {
        return true;
    }
    ch != UI2_OFFLINE_APP_FALLBACK_TWEMOJI
        && super::ui2_win_deco::push_chrome_twemoji_sprite64_placement(
            out,
            UI2_OFFLINE_APP_FALLBACK_TWEMOJI,
            rect,
        )
}

pub(super) fn collect_offline_dock_solid_rects(
    state: &Ui2State,
    out: &mut Vec<crate::intel::gpgpu::GpgpuSolidRect>,
) -> usize {
    let before = out.len();
    let pills = collect_offline_pill_slots(state);
    if pills.is_empty() {
        sync_offline_pill_hit_slots(&pills);
        return 0;
    }

    for pill in pills.iter() {
        let _ = super::ui2_win_deco::push_chrome_solid_outline(
            out,
            pill.rect,
            UI2_OFFLINE_PILL_LINE,
            2.0,
        );
    }
    sync_offline_pill_hit_slots(&pills);
    out.len().saturating_sub(before)
}

pub(super) fn collect_offline_dock_gradient_rects(
    state: &Ui2State,
    out: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) -> usize {
    let before = out.len();
    let pills = collect_offline_pill_slots(state);
    if pills.is_empty() {
        sync_offline_pill_hit_slots(&pills);
        return 0;
    }

    for pill in pills.iter() {
        let _ = super::ui2_win_deco::push_chrome_gradient_rect(
            out,
            pill.rect,
            UI2_OFFLINE_PILL_GRAD_LEFT,
            UI2_OFFLINE_PILL_GRAD_RIGHT,
            false,
        );
    }
    sync_offline_pill_hit_slots(&pills);
    out.len().saturating_sub(before)
}

pub(super) fn collect_offline_dock_hover_gradient_rects(
    state: &Ui2State,
    out: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) -> usize {
    let before = out.len();
    for cursor in &state.cursors {
        let Some(hover_rect) = cursor.hover_offline_dock_play_rect else {
            continue;
        };
        let _ = super::ui2_win_deco::push_chrome_gradient_rect(
            out,
            hover_rect,
            (0x00, 0x00, 0x00, 0xFF),
            (0xFF, 0xFF, 0xFF, 0xFF),
            true,
        );
    }
    out.len().saturating_sub(before)
}

pub(super) fn collect_offline_dock_sprite64_placements(
    state: &Ui2State,
    out: &mut Vec<crate::intel::gpgpu::GpgpuTwemojiSprite64Placement>,
) -> usize {
    let before = out.len();
    let pills = collect_offline_pill_slots(state);
    if pills.is_empty() {
        sync_offline_pill_hit_slots(&pills);
        return 0;
    }

    for pill in pills.iter() {
        let icon_rect = Ui2Rect::new(
            pill.rect.x,
            pill.rect.y,
            UI2_OFFLINE_PILL_PLAY_SIZE,
            UI2_OFFLINE_PILL_PLAY_SIZE,
        );
        let _ = push_offline_twemoji_sprite64_placement(
            out,
            offline_pill_icon_char(state, pill),
            icon_rect,
        );

        let play_rect = offline_pill_play_rect(pill);
        let _ = push_offline_twemoji_sprite64_placement(out, UI2_OFFLINE_PLAY_TWEMOJI, play_rect);
    }
    sync_offline_pill_hit_slots(&pills);
    out.len().saturating_sub(before)
}

/// Draw the offline-task dock on the right side and populate OFFLINE_PILLS for hit-testing.
pub(super) fn draw_offline_dock(state: &Ui2State) {
    if crate::gfx::is_intel_active() {
        let pills = collect_offline_pill_slots(state);
        sync_offline_pill_hit_slots(&pills);
        return;
    }

    let pills = collect_offline_pill_slots(state);
    if pills.is_empty() {
        sync_offline_pill_hit_slots(&pills);
        return;
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

        // App icon (left side of pill).
        let icon_rect = Ui2Rect::new(
            pill.rect.x,
            pill.rect.y,
            UI2_OFFLINE_PILL_PLAY_SIZE,
            UI2_OFFLINE_PILL_PLAY_SIZE,
        );
        if !draw_window_twemoji_button_standalone(
            state,
            icon_rect,
            offline_pill_icon_char(state, pill),
            0xFF,
        ) {
            let _ = draw_window_twemoji_button_standalone(
                state,
                icon_rect,
                UI2_OFFLINE_APP_FALLBACK_TWEMOJI,
                0xFF,
            );
        }

        // Title text (between app icon and play button).
        let title = offline_pill_title(state, pill);
        let title_rect = Ui2Rect::new(
            pill.rect.x + UI2_OFFLINE_PILL_PLAY_SIZE + 4.0,
            pill.rect.y,
            (pill.rect.w - (UI2_OFFLINE_PILL_PLAY_SIZE * 2.0) - 8.0).max(1.0),
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
        let play_rect = offline_pill_play_rect(pill);
        if state
            .cursors
            .iter()
            .any(|cursor| cursor.hover_offline_dock_play_rect == Some(play_rect))
        {
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                play_rect.x,
                play_rect.y,
                play_rect.w,
                play_rect.h,
                (0x00, 0x00, 0x00, 0x40),
                state.view_w,
                state.view_h,
            );
        }
        draw_window_twemoji_button_standalone(state, play_rect, UI2_OFFLINE_PLAY_TWEMOJI, 0xFF);
    }

    sync_offline_pill_hit_slots(&pills);
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
