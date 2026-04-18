#![allow(dead_code)]

use super::*;

const UI2_CHROME_TEXT_RGBA: (u8, u8, u8, u8) = (0x00, 0x00, 0x00, 0xFF);
const UI2_CHROME_TITLE_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;
const UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_UNLOCKED_TWEMOJI: char = '\u{25FC}';
const UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_LOCKED_TWEMOJI: char = '\u{25FB}';
const UI2_SYSTEM_BUTTON_FORK_TWEMOJI: char = '\u{2795}';
const UI2_SYSTEM_BUTTON_MINIMIZE_TWEMOJI: char = '\u{2796}';
const UI2_SYSTEM_BUTTON_MAXIMIZE_TWEMOJI: char = '\u{23F9}';
const UI2_SYSTEM_BUTTON_RESTORE_TWEMOJI: char = '\u{23CF}';
const UI2_SYSTEM_BUTTON_CLOSE_TWEMOJI: char = '\u{23EF}';
const UI2_RESIZE_HANDLE_TWEMOJI: char = '\u{2733}';

fn title_text_with_ellipsis(text: &str, max_width_px: f32) -> alloc::string::String {
    if text.is_empty() || max_width_px <= 0.0 {
        return alloc::string::String::new();
    }

    if (ui2_font_measure_text(UI2_CHROME_TITLE_FONT_TIER, text).width_px as f32) <= max_width_px {
        return alloc::string::String::from(text);
    }

    const ELLIPSIS: &str = "...";
    let ellipsis_w = ui2_font_measure_text(UI2_CHROME_TITLE_FONT_TIER, ELLIPSIS).width_px as f32;
    if ellipsis_w > max_width_px {
        return alloc::string::String::new();
    }

    let mut out = alloc::string::String::new();
    let mut used_w = 0.0f32;
    for ch in text.chars() {
        let ch_w = f32::from(ui2_font_char_advance_px(UI2_CHROME_TITLE_FONT_TIER, ch).max(1));
        if used_w + ch_w + ellipsis_w > max_width_px {
            break;
        }
        out.push(ch);
        used_w += ch_w;
    }
    out.push_str(ELLIPSIS);
    out
}

fn decoration_label_draw_rect(
    window: &Ui2Window,
    kind: Ui2DecorationLabelKind,
    anchor: Ui2Rect,
) -> Option<Ui2Rect> {
    let ch = kind.text(window).chars().next()?;
    let glyph = ui2_font_resolve_glyph(Ui2FontTier::Half, ch)?;
    Some(ui2_font_place_glyph_center(&glyph, anchor))
}

#[inline]
fn decoration_icon_draw_rect(anchor: Ui2Rect, right_align: bool) -> Ui2Rect {
    let side = anchor.w.min(anchor.h).max(1.0);
    let x = if right_align {
        anchor.x + (anchor.w - side).max(0.0)
    } else {
        anchor.x + ((anchor.w - side) * 0.5).max(0.0)
    };
    let y = anchor.y + ((anchor.h - side) * 0.5).max(0.0);
    Ui2Rect::new(x, y, side, side)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Ui2DecorationIconKind {
    SystemButton(Ui2SystemButtonAction),
    TitlebarWindow,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Ui2DecorationLabelKind {
    SystemButton(Ui2SystemButtonAction),
}

impl Ui2DecorationLabelKind {
    #[inline]
    fn text(self, window: &Ui2Window) -> &'static str {
        match self {
            Self::SystemButton(Ui2SystemButtonAction::ToggleComposition) => {
                if window.composition_locked {
                    "▣"
                } else {
                    "■"
                }
            }
            Self::SystemButton(Ui2SystemButtonAction::Fork) => "+",
            Self::SystemButton(Ui2SystemButtonAction::Minimize) => "⊟",
            Self::SystemButton(Ui2SystemButtonAction::Restore) => "⏏",
            Self::SystemButton(Ui2SystemButtonAction::ToggleMaximize) => "□",
            Self::SystemButton(Ui2SystemButtonAction::Close) => "⊠",
        }
    }
}

trait Ui2DecorationIconSource {
    fn icon_id(self, window: &Ui2Window) -> u32;
}

impl Ui2DecorationIconSource for Ui2DecorationIconKind {
    fn icon_id(self, window: &Ui2Window) -> u32 {
        match self {
            Self::SystemButton(Ui2SystemButtonAction::ToggleComposition) => 0,
            Self::SystemButton(Ui2SystemButtonAction::Fork) => UI2_SYSTEM_BUTTON_FORK_ICON_ID,
            Self::SystemButton(Ui2SystemButtonAction::Minimize) => {
                UI2_SYSTEM_BUTTON_MINIMIZE_ICON_ID
            }
            Self::SystemButton(Ui2SystemButtonAction::Restore) => 0,
            Self::SystemButton(Ui2SystemButtonAction::ToggleMaximize) => {
                UI2_SYSTEM_BUTTON_MAXIMIZE_ICON_ID
            }
            Self::SystemButton(Ui2SystemButtonAction::Close) => UI2_SYSTEM_BUTTON_CLOSE_ICON_ID,
            Self::TitlebarWindow => window.icon_id,
        }
    }
}

pub(super) fn draw_window_decoration_icon(
    state: &Ui2State,
    window: &Ui2Window,
    kind: Ui2DecorationIconKind,
    x: f32,
    y: f32,
    side_px: f32,
) {
    let icon_id = kind.icon_id(window);
    if icon_id == 0 {
        return;
    }

    let _ = crate::gfx::lyon::draw_lyon_icon_alpha_scaled_no_present(
        icon_id,
        0,
        0,
        x,
        y,
        side_px,
        state.view_w,
        state.view_h,
        window.alpha,
    );
}

fn draw_window_title_texture_icon(
    state: &Ui2State,
    window: &Ui2Window,
    x: f32,
    y: f32,
    side_px: f32,
) {
    if !texture_is_drawable(window.title_icon_tex_id) {
        return;
    }
    let _ = draw_texture_rect_no_present(
        window.title_icon_tex_id,
        x,
        y,
        side_px,
        side_px,
        state.view_w,
        state.view_h,
        true,
        window.alpha,
    );
}

fn draw_window_decoration_label(
    state: &Ui2State,
    window: &Ui2Window,
    kind: Ui2DecorationLabelKind,
    rect: Ui2Rect,
) {
    let Some(ch) = kind.text(window).chars().next() else {
        return;
    };
    let Some(glyph) = ui2_font_resolve_glyph(Ui2FontTier::Half, ch) else {
        return;
    };
    if !glyph.ready {
        return;
    }
    let Some(texture) = glyph.texture else {
        return;
    };

    let draw_rect = decoration_label_draw_rect(window, kind, rect).unwrap_or(rect);
    let glyph_w = draw_rect.w;
    let glyph_h = draw_rect.h;
    let glyph_x = draw_rect.x;
    let glyph_y = draw_rect.y;
    let atlas_w = f32::from(glyph.region.atlas_w.max(1));
    let atlas_h = f32::from(glyph.region.atlas_h.max(1));
    let src_x = f32::from(glyph.region.src_x);
    let src_y = f32::from(glyph.region.src_y);
    let _ = draw_texture_rect_uv_rgba_no_present(
        texture.tex_id,
        glyph_x,
        glyph_y,
        glyph_w,
        glyph_h,
        src_x / atlas_w,
        src_y / atlas_h,
        (src_x + f32::from(glyph.region.src_w)) / atlas_w,
        (src_y + f32::from(glyph.region.src_h)) / atlas_h,
        state.view_w,
        state.view_h,
        true,
        modulate_rgba_alpha(UI2_CHROME_TEXT_RGBA, window.alpha),
    );
}

fn draw_window_system_button(state: &Ui2State, window: &Ui2Window, action: Ui2SystemButtonAction) {
    if window.state == Ui2WindowStateKind::Minimized
        && action != Ui2SystemButtonAction::ToggleMaximize
        && action != Ui2SystemButtonAction::Restore
        && action != Ui2SystemButtonAction::Close
    {
        return;
    }
    if window.state == Ui2WindowStateKind::Maximized
        && action == Ui2SystemButtonAction::ToggleMaximize
    {
        return;
    }
    let Some(rect) = window_system_button_rect(state, window, action) else {
        return;
    };
    if let Some(ch) = window_system_button_twemoji(window, action) {
        if draw_window_twemoji_button(state, window, rect, ch) {
            return;
        }
    }
    let kind = Ui2DecorationIconKind::SystemButton(action);
    if kind.icon_id(window) != 0 {
        let draw = decoration_icon_draw_rect(rect, false);
        draw_window_decoration_icon(state, window, kind, draw.x, draw.y, draw.w);
    } else {
        draw_window_decoration_label(
            state,
            window,
            Ui2DecorationLabelKind::SystemButton(action),
            rect,
        );
    }
}

fn draw_window_twemoji_button(
    state: &Ui2State,
    window: &Ui2Window,
    rect: Ui2Rect,
    ch: char,
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
        (255, 255, 255, window.alpha),
    )
}

#[inline]
fn window_system_button_twemoji(window: &Ui2Window, action: Ui2SystemButtonAction) -> Option<char> {
    match action {
        Ui2SystemButtonAction::ToggleComposition => Some(if window.composition_locked {
            UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_LOCKED_TWEMOJI
        } else {
            UI2_SYSTEM_BUTTON_TOGGLE_COMPOSITION_UNLOCKED_TWEMOJI
        }),
        Ui2SystemButtonAction::Fork => Some(UI2_SYSTEM_BUTTON_FORK_TWEMOJI),
        Ui2SystemButtonAction::Minimize => Some(UI2_SYSTEM_BUTTON_MINIMIZE_TWEMOJI),
        Ui2SystemButtonAction::Restore => Some(UI2_SYSTEM_BUTTON_RESTORE_TWEMOJI),
        Ui2SystemButtonAction::ToggleMaximize => Some(UI2_SYSTEM_BUTTON_MAXIMIZE_TWEMOJI),
        Ui2SystemButtonAction::Close => Some(UI2_SYSTEM_BUTTON_CLOSE_TWEMOJI),
    }
}

fn draw_window_bottom_resize_button(state: &Ui2State, window: &Ui2Window) {
    let Some(rect) = window_bottom_resize_button_rect(state, window) else {
        return;
    };
    draw_window_twemoji_button(state, window, rect, UI2_RESIZE_HANDLE_TWEMOJI);
}

fn draw_window_system_scrollbars(state: &Ui2State, window: &Ui2Window) {
    let track = (0xEA, 0xEC, 0xEF, 0xFF);
    let thumb = (0xB6, 0xBC, 0xC4, 0xFF);
    let inset = (0xD7, 0xDB, 0xE1, 0xFF);

    if let Some(vbar) = window_vertical_scrollbar_rect(state, window) {
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            vbar.x,
            vbar.y,
            vbar.w,
            vbar.h,
            track,
            state.view_w,
            state.view_h,
        );
        let thumb_h = if let Some(snapshot) = window_scroll_snapshot(window) {
            let viewport_h = snapshot.viewport_height.max(1) as f32;
            let content_h = snapshot.content_height.max(snapshot.viewport_height.max(1)) as f32;
            libm::fmaxf(10.0, (vbar.h * (viewport_h / content_h)).min(vbar.h))
        } else {
            libm::fminf(vbar.h, 18.0)
        };
        let thumb_y = if let Some(snapshot) = window_scroll_snapshot(window) {
            let scroll_range = hosted_browser_scroll_max(&snapshot) as f32;
            let avail = (vbar.h - thumb_h).max(0.0);
            if scroll_range > 0.0 {
                vbar.y
                    + (avail
                        * ((normalized_hosted_browser_scroll(&snapshot) as f32) / scroll_range))
            } else {
                vbar.y
            }
        } else {
            vbar.y
        };
        let thumb_x = vbar.x + 1.0;
        let thumb_w = (vbar.w - 2.0).max(1.0);
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            thumb_x,
            thumb_y,
            thumb_w,
            thumb_h,
            thumb,
            state.view_w,
            state.view_h,
        );
    }

    if let Some(hbar) = window_horizontal_scrollbar_rect(state, window) {
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            hbar.x,
            hbar.y,
            hbar.w,
            hbar.h,
            track,
            state.view_w,
            state.view_h,
        );
        let thumb_w = if let Some(snapshot) = window_scroll_snapshot(window) {
            let viewport_w = snapshot.viewport_width.max(1) as f32;
            let content_w = snapshot.content_width.max(snapshot.viewport_width.max(1)) as f32;
            libm::fmaxf(10.0, (hbar.w * (viewport_w / content_w)).min(hbar.w))
        } else {
            libm::fminf((hbar.w - 2.0).max(8.0), 18.0)
        };
        let thumb_x = if let Some(snapshot) = window_scroll_snapshot(window) {
            let scroll_range = hosted_browser_scroll_x_max(&snapshot) as f32;
            let avail = (hbar.w - thumb_w).max(0.0);
            if scroll_range > 0.0 {
                hbar.x
                    + (avail
                        * ((normalized_hosted_browser_scroll_x(&snapshot) as f32) / scroll_range))
            } else {
                hbar.x
            }
        } else {
            hbar.x + ((hbar.w - thumb_w) * 0.5)
        };
        let thumb_y = hbar.y + 1.0;
        let thumb_h = (hbar.h - 2.0).max(1.0);
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            thumb_x,
            thumb_y,
            thumb_w,
            thumb_h,
            inset,
            state.view_w,
            state.view_h,
        );
    }
}

pub(super) fn draw_window_chrome(state: &Ui2State, window: &Ui2Window, rect: Ui2Rect) {
    const TITLE_TEXT_H: f32 = 24.0;
    const GL_ONE: u32 = 1;
    const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;

    let frame_base_rgba = (0xD9, 0xDE, 0xE5, 0xFF);
    let frame_left_rgba = blend_rgba_over((0x00, 0x00, 0x00, 0x52), frame_base_rgba);
    let frame_mid_rgba = frame_base_rgba;
    let frame_right_rgba = blend_rgba_over((0xFF, 0xFF, 0xFF, 0x52), frame_base_rgba);
    let frame_left_rgba = modulate_rgba_alpha(frame_left_rgba, window.alpha);
    let frame_mid_rgba = modulate_rgba_alpha(frame_mid_rgba, window.alpha);
    let frame_right_rgba = modulate_rgba_alpha(frame_right_rgba, window.alpha);
    let body_rgba = (0xFB, 0xFB, 0xF8, window.alpha);
    let titleband_h =
        if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
            if window.state == Ui2WindowStateKind::Minimized {
                rect.h
            } else {
                UI2_TITLE_H.min(rect.h)
            }
        } else {
            0.0
        };
    let body_y = rect.y + titleband_h;
    let body_h = (rect.h - titleband_h).max(0.0);
    if titleband_h > 0.0 {
        let _ = crate::gfx::lyon::draw_horizontal_four_stop_rect_no_present(
            rect.x,
            rect.y,
            rect.w,
            titleband_h,
            frame_mid_rgba,
            frame_left_rgba,
            frame_mid_rgba,
            frame_right_rgba,
            state.view_w,
            state.view_h,
        );
    }
    if body_h > 0.0 {
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x,
            body_y,
            rect.w,
            body_h,
            body_rgba,
            state.view_w,
            state.view_h,
        );
    }
    if let Some(bottom_bar) = window_bottom_bar_rect(state, window) {
        let _ = crate::gfx::lyon::draw_horizontal_three_stop_rect_no_present(
            bottom_bar.x,
            bottom_bar.y,
            bottom_bar.w,
            bottom_bar.h,
            frame_left_rgba,
            frame_mid_rgba,
            frame_right_rgba,
            0.5,
            state.view_w,
            state.view_h,
        );
    }
    if !window.selected_cursor_slots.is_empty() {
        let bar_span = if window.decoration_mode == Ui2WindowDecorationMode::System
            && window.titlebar_visible
        {
            window_decoration_rect(state, window).unwrap_or(rect)
        } else {
            rect
        };
        let bar_x = bar_span.x;
        let bar_y = bar_span.y - 2.0;
        let bar_w = bar_span.w.max(0.0);
        let bar_h = 2.0;
        let cursor_count = window.selected_cursor_slots.len() as f32;
        if bar_w > 0.0 && cursor_count > 0.0 {
            for (idx, slot_id) in window.selected_cursor_slots.iter().enumerate() {
                let seg_x = bar_x + (bar_w * (idx as f32 / cursor_count));
                let seg_right = if idx + 1 == window.selected_cursor_slots.len() {
                    bar_x + bar_w
                } else {
                    bar_x + (bar_w * ((idx + 1) as f32 / cursor_count))
                };
                let seg_w = (seg_right - seg_x).max(0.0);
                if seg_w <= 0.0 {
                    continue;
                }
                let selection_rgba =
                    modulate_rgba_alpha(super::ui2_hid::cursor_color(*slot_id), window.alpha);
                let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                    seg_x,
                    bar_y,
                    seg_w,
                    bar_h,
                    selection_rgba,
                    state.view_w,
                    state.view_h,
                );
            }
        }
    }

    if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
        let has_title_texture_icon = texture_is_drawable(window.title_icon_tex_id);
        let has_title_twemoji =
            !has_title_texture_icon && window.icon_id == 0 && window.title_twemoji != '\0';
        if has_title_texture_icon || window.icon_id != 0 || has_title_twemoji {
            let icon_side = 16.0f32;
            let icon_x = rect.x + 8.0;
            let icon_y = rect.y + ((titleband_h - icon_side) * 0.5).max(0.0);
            if has_title_texture_icon {
                draw_window_title_texture_icon(state, window, icon_x, icon_y, icon_side);
            } else if has_title_twemoji {
                draw_window_twemoji_button(
                    state,
                    window,
                    Ui2Rect::new(icon_x, icon_y, icon_side, icon_side),
                    window.title_twemoji,
                );
            } else {
                draw_window_decoration_icon(
                    state,
                    window,
                    Ui2DecorationIconKind::TitlebarWindow,
                    icon_x,
                    icon_y,
                    icon_side,
                );
            }
        }
        let title_left = if has_title_texture_icon || window.icon_id != 0 || has_title_twemoji {
            rect.x + 28.0
        } else {
            rect.x + 8.0
        };
        let title_right = [
            Ui2SystemButtonAction::ToggleComposition,
            Ui2SystemButtonAction::Fork,
            Ui2SystemButtonAction::Minimize,
            Ui2SystemButtonAction::Restore,
            Ui2SystemButtonAction::ToggleMaximize,
            Ui2SystemButtonAction::Close,
        ]
        .into_iter()
        .filter_map(|action| window_system_button_rect(state, window, action))
        .map(|button| button.x - 8.0)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal))
        .unwrap_or(rect.x + rect.w - 8.0);
        let title_rect = Ui2Rect::new(
            title_left,
            rect.y,
            (title_right - title_left).max(0.0),
            titleband_h.max(1.0),
        );
        let title_text = title_text_with_ellipsis(window.title.as_str(), title_rect.w);
        let _ = ui2_font_draw_text_line_in_rect_with_tier_rgba_no_present(
            title_text.as_str(),
            title_rect,
            UI2_CHROME_TITLE_FONT_TIER,
            Ui2FontTextAlign::Left,
            Ui2FontVerticalAlign::Center,
            state.view_w,
            state.view_h,
            modulate_rgba_alpha(UI2_CHROME_TEXT_RGBA, window.alpha),
        );
        draw_window_system_button(state, window, Ui2SystemButtonAction::ToggleComposition);
        draw_window_system_button(state, window, Ui2SystemButtonAction::Fork);
        draw_window_system_button(state, window, Ui2SystemButtonAction::Minimize);
        draw_window_system_button(state, window, Ui2SystemButtonAction::Restore);
        draw_window_system_button(state, window, Ui2SystemButtonAction::ToggleMaximize);
        draw_window_system_button(state, window, Ui2SystemButtonAction::Close);
    }
    if window.decoration_mode == Ui2WindowDecorationMode::System {
        draw_window_system_scrollbars(state, window);
        draw_window_bottom_resize_button(state, window);
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowDecorationMode {
    System = 0,
    Client = 1,
    None = 2,
}

impl Ui2WindowDecorationMode {
    #[inline]
    pub(super) const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::System),
            1 => Some(Self::Client),
            2 => Some(Self::None),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowVerticalScrollbarSide {
    Left = 0,
    Right = 1,
}

impl Ui2WindowVerticalScrollbarSide {
    #[inline]
    pub(super) const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowHorizontalScrollbarSide {
    Top = 0,
    Bottom = 1,
}

impl Ui2WindowHorizontalScrollbarSide {
    #[inline]
    pub(super) const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Top),
            1 => Some(Self::Bottom),
            _ => None,
        }
    }
}

pub fn set_window_decorations(id: u32, mode: Ui2WindowDecorationMode) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.decoration_mode == mode {
        return true;
    }
    window.decoration_mode = mode;
    state.compose_reason = "decor-window";
    let noted = note_window_dirty(&mut state, id, "decor-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn set_window_titlebar_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.titlebar_visible == visible {
        return true;
    }
    window.titlebar_visible = visible;
    state.compose_reason = "decor-titlebar-window";
    let noted = note_window_dirty(&mut state, id, "decor-titlebar-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
        if !visible {
            clear_window_drag_claims(&mut state, id);
        }
    }
    noted
}

pub fn set_window_bottom_bar_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.bottom_bar_visible == visible {
        return true;
    }
    window.bottom_bar_visible = visible;
    state.compose_reason = "decor-bottombar-window";
    let noted = note_window_dirty(&mut state, id, "decor-bottombar-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
        if !visible {
            clear_window_drag_claims(&mut state, id);
        }
    }
    noted
}

pub fn set_window_left_scrollbar_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.left_scrollbar_visible == visible {
        return true;
    }
    window.left_scrollbar_visible = visible;
    state.compose_reason = "decor-left-scrollbar-window";
    let noted = note_window_dirty(&mut state, id, "decor-left-scrollbar-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
        if !visible {
            clear_window_drag_claims(&mut state, id);
        }
    }
    noted
}

pub fn set_window_bottom_scrollbar_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.bottom_scrollbar_visible == visible {
        return true;
    }
    window.bottom_scrollbar_visible = visible;
    state.compose_reason = "decor-bottom-scrollbar-window";
    let noted = note_window_dirty(&mut state, id, "decor-bottom-scrollbar-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
        if !visible {
            clear_window_drag_claims(&mut state, id);
        }
    }
    noted
}

pub fn set_window_vertical_scrollbar_side(id: u32, side: Ui2WindowVerticalScrollbarSide) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.vertical_scrollbar_side == side {
        return true;
    }
    window.vertical_scrollbar_side = side;
    state.compose_reason = "decor-vertical-scrollbar-side-window";
    let noted = note_window_dirty(&mut state, id, "decor-vertical-scrollbar-side-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
        clear_window_drag_claims(&mut state, id);
    }
    noted
}

pub fn set_window_horizontal_scrollbar_side(
    id: u32,
    side: Ui2WindowHorizontalScrollbarSide,
) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.horizontal_scrollbar_side == side {
        return true;
    }
    window.horizontal_scrollbar_side = side;
    state.compose_reason = "decor-horizontal-scrollbar-side-window";
    let noted = note_window_dirty(&mut state, id, "decor-horizontal-scrollbar-side-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub(super) fn window_decoration_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.titlebar_visible {
        return None;
    }
    let rect = effective_window_rect(state, window);
    let h = if window.state == Ui2WindowStateKind::Minimized {
        rect.h
    } else {
        UI2_TITLE_H
    };
    if !(rect.w > 0.0 && h > 0.0) {
        return None;
    }
    Some(Ui2Rect::new(rect.x, rect.y, rect.w, h))
}

#[inline]
fn window_titlebar_height(window: &Ui2Window) -> f32 {
    if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
        UI2_TITLE_H
    } else {
        0.0
    }
}

#[inline]
fn window_bottom_bar_height(window: &Ui2Window) -> f32 {
    if window.decoration_mode == Ui2WindowDecorationMode::System && window.bottom_bar_visible {
        UI2_BOTTOM_BAR_H
    } else {
        0.0
    }
}

#[inline]
fn window_vertical_scrollbar_width(window: &Ui2Window) -> f32 {
    if window.decoration_mode == Ui2WindowDecorationMode::System && window.left_scrollbar_visible {
        UI2_SYSTEM_SCROLLBAR_PX
    } else {
        0.0
    }
}

#[inline]
fn window_horizontal_scrollbar_height(window: &Ui2Window) -> f32 {
    if window.decoration_mode == Ui2WindowDecorationMode::System && window.bottom_scrollbar_visible
    {
        UI2_SYSTEM_SCROLLBAR_PX
    } else {
        0.0
    }
}

#[inline]
fn window_left_inset(window: &Ui2Window) -> f32 {
    if window.vertical_scrollbar_side == Ui2WindowVerticalScrollbarSide::Left {
        window_vertical_scrollbar_width(window)
    } else {
        0.0
    }
}

#[inline]
fn window_right_inset(window: &Ui2Window) -> f32 {
    if window.vertical_scrollbar_side == Ui2WindowVerticalScrollbarSide::Right {
        window_vertical_scrollbar_width(window)
    } else {
        0.0
    }
}

#[inline]
fn window_top_inset(window: &Ui2Window) -> f32 {
    let mut inset = window_titlebar_height(window);
    if window.horizontal_scrollbar_side == Ui2WindowHorizontalScrollbarSide::Top {
        inset += window_horizontal_scrollbar_height(window);
    }
    inset
}

#[inline]
fn window_bottom_inset(window: &Ui2Window) -> f32 {
    let mut inset = window_bottom_bar_height(window);
    if window.horizontal_scrollbar_side == Ui2WindowHorizontalScrollbarSide::Bottom {
        inset += window_horizontal_scrollbar_height(window);
    }
    inset
}

pub(super) fn window_bottom_bar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.bottom_bar_visible {
        return None;
    }
    if window.state == Ui2WindowStateKind::Minimized {
        return None;
    }
    let rect = effective_window_rect(state, window);
    let bar_h = window_bottom_bar_height(window);
    if !(rect.w > 0.0 && rect.h > bar_h) {
        return None;
    }
    Some(Ui2Rect::new(rect.x, rect.y + rect.h - bar_h, rect.w, bar_h))
}

fn window_bottom_scrollbar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.bottom_scrollbar_visible
    {
        return None;
    }
    if window.state == Ui2WindowStateKind::Minimized {
        return None;
    }
    let rect = effective_window_rect(state, window);
    let title_h = window_titlebar_height(window);
    let bottom_bar_h = window_bottom_bar_height(window);
    let scrollbar_h = window_horizontal_scrollbar_height(window);
    if !(rect.w > 0.0 && rect.h > (title_h + bottom_bar_h + scrollbar_h)) {
        return None;
    }
    let y = if window.horizontal_scrollbar_side == Ui2WindowHorizontalScrollbarSide::Top {
        rect.y + title_h
    } else {
        rect.y + rect.h - bottom_bar_h - scrollbar_h
    };
    Some(Ui2Rect::new(rect.x, y, rect.w.max(1.0), scrollbar_h))
}

pub(super) fn window_content_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    if window.state == Ui2WindowStateKind::Minimized {
        return None;
    }
    let rect = effective_window_rect(state, window);
    match window.decoration_mode {
        Ui2WindowDecorationMode::System => {
            let left_inset = window_left_inset(window);
            let right_inset = window_right_inset(window);
            let top_inset = window_top_inset(window);
            let bottom_inset = window_bottom_inset(window);
            let w = rect.w - left_inset - right_inset;
            let h = rect.h - top_inset - bottom_inset;
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            Some(Ui2Rect::new(rect.x + left_inset, rect.y + top_inset, w, h))
        }
        Ui2WindowDecorationMode::Client => {
            let w = rect.w.max(1.0);
            let h = rect.h.max(1.0);
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            Some(Ui2Rect::new(rect.x, rect.y, w, h))
        }
        Ui2WindowDecorationMode::None => {
            if !(rect.w > 0.0 && rect.h > 0.0) {
                return None;
            }
            Some(rect)
        }
    }
}

pub(super) fn window_vertical_scrollbar_rect(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.left_scrollbar_visible {
        return None;
    }
    if window.state == Ui2WindowStateKind::Minimized {
        return None;
    }
    let rect = effective_window_rect(state, window);
    let top_inset = window_top_inset(window);
    let bottom_inset = window_bottom_inset(window);
    let w = window_vertical_scrollbar_width(window);
    let h = rect.h - top_inset - bottom_inset;
    if !(h > 0.0) {
        return None;
    }
    let x = if window.vertical_scrollbar_side == Ui2WindowVerticalScrollbarSide::Left {
        rect.x
    } else {
        rect.x + rect.w - w
    };
    Some(Ui2Rect::new(x, rect.y + top_inset, w, h))
}

pub(super) fn window_horizontal_scrollbar_rect(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<Ui2Rect> {
    window_bottom_scrollbar_rect(state, window)
}

pub(super) fn window_bottom_resize_button_rect(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<Ui2Rect> {
    window_bottom_resize_button_anchor_rect(state, window)
}

fn window_bottom_resize_button_anchor_rect(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.bottom_bar_visible {
        return None;
    }
    if window.state != Ui2WindowStateKind::Normal {
        return None;
    }
    let bar = window_bottom_bar_rect(state, window)?;
    let button_size = bar.h;
    let button_x = bar.x + bar.w - button_size;
    let button_y = bar.y;
    Some(Ui2Rect::new(button_x, button_y, button_size, button_size))
}

pub(super) fn window_system_button_rect(
    state: &Ui2State,
    window: &Ui2Window,
    action: Ui2SystemButtonAction,
) -> Option<Ui2Rect> {
    window_system_button_anchor_rect(state, window, action)
}

fn window_system_button_anchor_rect(
    state: &Ui2State,
    window: &Ui2Window,
    action: Ui2SystemButtonAction,
) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.titlebar_visible {
        return None;
    }
    let titlebar = window_decoration_rect(state, window)?;
    let s = titlebar.h.max(1.0);
    let gap = 1.0f32;

    // Right-to-left order: Close, Maximize, Minimize, Fork, Composition.
    // For minimized windows only Close + Maximize are shown.
    let actions_normal: &[Ui2SystemButtonAction] = &[
        Ui2SystemButtonAction::Close,
        Ui2SystemButtonAction::ToggleMaximize,
        Ui2SystemButtonAction::Minimize,
        Ui2SystemButtonAction::Fork,
        Ui2SystemButtonAction::ToggleComposition,
    ];
    let actions_minimized: &[Ui2SystemButtonAction] = &[
        Ui2SystemButtonAction::Close,
        Ui2SystemButtonAction::Restore,
        Ui2SystemButtonAction::ToggleMaximize,
    ];
    let actions_maximized: &[Ui2SystemButtonAction] = &[
        Ui2SystemButtonAction::Close,
        Ui2SystemButtonAction::Minimize,
        Ui2SystemButtonAction::Fork,
        Ui2SystemButtonAction::ToggleComposition,
    ];
    let actions = match window.state {
        Ui2WindowStateKind::Minimized => actions_minimized,
        Ui2WindowStateKind::Maximized => actions_maximized,
        _ => actions_normal,
    };
    let slot = actions.iter().position(|a| *a == action)?;
    let x = titlebar.x + titlebar.w - (slot as f32 + 1.0) * s - slot as f32 * gap;
    Some(Ui2Rect::new(x, titlebar.y, s, s))
}

pub(super) fn system_button_action_at(
    state: &Ui2State,
    window_id: u32,
    x: f32,
    y: f32,
) -> Option<Ui2SystemButtonAction> {
    let window = state.windows.iter().find(|window| window.id == window_id)?;
    for action in [
        Ui2SystemButtonAction::ToggleComposition,
        Ui2SystemButtonAction::Fork,
        Ui2SystemButtonAction::Minimize,
        Ui2SystemButtonAction::Restore,
        Ui2SystemButtonAction::ToggleMaximize,
        Ui2SystemButtonAction::Close,
    ] {
        let Some(rect) = window_system_button_rect(state, window, action) else {
            continue;
        };
        if rect_contains_point(rect, x, y) {
            return Some(action);
        }
    }
    None
}

pub(super) fn window_rect_for_content(
    mode: Ui2WindowDecorationMode,
    content_rect: Ui2Rect,
) -> Ui2Rect {
    match mode {
        Ui2WindowDecorationMode::System => Ui2Rect::new(
            content_rect.x - UI2_SYSTEM_SCROLLBAR_PX,
            content_rect.y - UI2_TITLE_H,
            content_rect.w + UI2_SYSTEM_SCROLLBAR_PX,
            content_rect.h + UI2_TITLE_H + UI2_SYSTEM_SCROLLBAR_PX + UI2_BOTTOM_BAR_H,
        ),
        Ui2WindowDecorationMode::Client => content_rect,
        Ui2WindowDecorationMode::None => content_rect,
    }
}
