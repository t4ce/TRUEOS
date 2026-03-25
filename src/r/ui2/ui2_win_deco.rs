#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

use super::*;

const UI2_DECOR_SHARED_ICON_TEX_BASE_ID: u32 = 4_720;
const UI2_DECOR_WINDOW_ICON_TEX_BASE_ID: u32 = 4_800;
const UI2_DECOR_WINDOW_ICON_TEX_STRIDE: u32 = 16;
const UI2_DECOR_ICON_SIDE_PX: u32 = 16;
const UI2_DECOR_ICON_SMALL_SET: u32 = 1;
const UI2_DECOR_ICON_COLOR_ID: u32 = 0;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2DecorationIconCacheScope {
    Shared,
    WindowLocal,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Ui2DecorationIconKind {
    SystemButton(Ui2SystemButtonAction),
    ResizeHandle,
    TitlebarWindow,
}

trait Ui2DecorationIconSource {
    fn icon_id(self, window: &Ui2Window) -> u32;
    fn cache_scope(self, window: &Ui2Window) -> Ui2DecorationIconCacheScope;
    fn repaint_reason(self) -> &'static str;
    fn shared_slot(self) -> Option<u32>;
}

impl Ui2DecorationIconSource for Ui2DecorationIconKind {
    fn icon_id(self, window: &Ui2Window) -> u32 {
        match self {
            Self::SystemButton(Ui2SystemButtonAction::Fork) => UI2_SYSTEM_BUTTON_FORK_ICON_ID,
            Self::SystemButton(Ui2SystemButtonAction::Minimize) => {
                UI2_SYSTEM_BUTTON_MINIMIZE_ICON_ID
            }
            Self::SystemButton(Ui2SystemButtonAction::ToggleMaximize) => {
                UI2_SYSTEM_BUTTON_MAXIMIZE_ICON_ID
            }
            Self::SystemButton(Ui2SystemButtonAction::Close) => UI2_SYSTEM_BUTTON_CLOSE_ICON_ID,
            Self::ResizeHandle => 1,
            Self::TitlebarWindow => window.icon_id,
        }
    }

    fn cache_scope(self, _window: &Ui2Window) -> Ui2DecorationIconCacheScope {
        match self {
            Self::TitlebarWindow => Ui2DecorationIconCacheScope::WindowLocal,
            Self::SystemButton(_) | Self::ResizeHandle => Ui2DecorationIconCacheScope::Shared,
        }
    }

    fn repaint_reason(self) -> &'static str {
        match self {
            Self::SystemButton(_) => "decor-system-icon-upload",
            Self::ResizeHandle => "decor-resize-icon-upload",
            Self::TitlebarWindow => "decor-title-icon-upload",
        }
    }

    fn shared_slot(self) -> Option<u32> {
        match self {
            Self::SystemButton(Ui2SystemButtonAction::Fork) => Some(0),
            Self::SystemButton(Ui2SystemButtonAction::Minimize) => Some(1),
            Self::SystemButton(Ui2SystemButtonAction::ToggleMaximize) => Some(2),
            Self::SystemButton(Ui2SystemButtonAction::Close) => Some(3),
            Self::ResizeHandle => Some(4),
            Self::TitlebarWindow => None,
        }
    }
}

#[inline]
fn ui2_decor_normalized_icon_id(icon_id: u32) -> u32 {
    const ICON_SHAPE_COUNT: u32 = 12;
    icon_id % ICON_SHAPE_COUNT
}

#[inline]
fn ui2_decor_icon_tex_id(kind: Ui2DecorationIconKind, window: &Ui2Window) -> Option<u32> {
    let icon_id = kind.icon_id(window);
    if icon_id == 0 {
        return None;
    }
    match kind.cache_scope(window) {
        Ui2DecorationIconCacheScope::Shared => kind
            .shared_slot()
            .map(|slot| UI2_DECOR_SHARED_ICON_TEX_BASE_ID.saturating_add(slot)),
        Ui2DecorationIconCacheScope::WindowLocal => Some(
            UI2_DECOR_WINDOW_ICON_TEX_BASE_ID
                .saturating_add(window.id.saturating_mul(UI2_DECOR_WINDOW_ICON_TEX_STRIDE))
                .saturating_add(
                    ui2_decor_normalized_icon_id(icon_id) % UI2_DECOR_WINDOW_ICON_TEX_STRIDE,
                ),
        ),
    }
}

fn queue_ui2_decor_icon_upload(
    kind: Ui2DecorationIconKind,
    window: &Ui2Window,
    tex_id: u32,
) -> bool {
    let icon_id = kind.icon_id(window);
    if icon_id == 0 {
        return false;
    }

    let need = unsafe {
        crate::gfx::lyon::trueos_cabi_gfx_bake_lyon_icon_rgba(
            icon_id,
            UI2_DECOR_ICON_COLOR_ID,
            UI2_DECOR_ICON_SMALL_SET,
            core::ptr::null_mut(),
            0,
        )
    };
    if need <= 0 {
        return false;
    }
    let need = need as usize;
    let expected = (UI2_DECOR_ICON_SIDE_PX as usize)
        .saturating_mul(UI2_DECOR_ICON_SIDE_PX as usize)
        .saturating_mul(4);
    if need != expected {
        return false;
    }

    let mut baked = vec![0u8; need];
    let wrote = unsafe {
        crate::gfx::lyon::trueos_cabi_gfx_bake_lyon_icon_rgba(
            icon_id,
            UI2_DECOR_ICON_COLOR_ID,
            UI2_DECOR_ICON_SMALL_SET,
            baked.as_mut_ptr(),
            baked.len(),
        )
    };
    if wrote != need as i32 {
        return false;
    }

    let mut rgba = Vec::with_capacity(need);
    for px in baked.chunks_exact(4) {
        rgba.extend_from_slice(&[0, 0, 0, px[0]]);
    }

    crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        tex_id,
        UI2_DECOR_ICON_SIDE_PX,
        UI2_DECOR_ICON_SIDE_PX,
        rgba.as_slice(),
        window.id,
        kind.repaint_reason(),
    )
}

pub(super) fn draw_window_decoration_icon(
    state: &Ui2State,
    window: &Ui2Window,
    kind: Ui2DecorationIconKind,
    x: f32,
    y: f32,
    side_px: f32,
) {
    let Some(tex_id) = ui2_decor_icon_tex_id(kind, window) else {
        return;
    };
    if texture_is_drawable(tex_id)
        && draw_texture_rect_no_present(
            tex_id,
            x,
            y,
            side_px,
            side_px,
            state.view_w,
            state.view_h,
            true,
            window.alpha,
        )
    {
        return;
    }

    let _ = queue_ui2_decor_icon_upload(kind, window, tex_id);

    let icon_id = kind.icon_id(window);
    if icon_id != 0 {
        let _ = crate::gfx::lyon::draw_lyon_icon_alpha_no_present(
            icon_id,
            UI2_DECOR_ICON_COLOR_ID,
            UI2_DECOR_ICON_SMALL_SET,
            x,
            y,
            state.view_w,
            state.view_h,
            window.alpha,
        );
    }
}

fn draw_window_system_button(state: &Ui2State, window: &Ui2Window, action: Ui2SystemButtonAction) {
    if window.state == Ui2WindowStateKind::Minimized
        && action != Ui2SystemButtonAction::ToggleMaximize
    {
        return;
    }
    let Some(rect) = window_system_button_rect(state, window, action) else {
        return;
    };

    let icon_x = rect.x + ((rect.w - 16.0) * 0.5);
    let icon_y = rect.y + ((rect.h - 16.0) * 0.5);
    draw_window_decoration_icon(
        state,
        window,
        Ui2DecorationIconKind::SystemButton(action),
        icon_x,
        icon_y,
        16.0,
    );
}

fn draw_window_bottom_resize_button(state: &Ui2State, window: &Ui2Window) {
    let Some(rect) = window_bottom_resize_button_rect(state, window) else {
        return;
    };
    let icon_side = 16.0f32;
    let icon_x = rect.x + ((rect.w - icon_side) * 0.5);
    let icon_y = rect.y + ((rect.h - icon_side) * 0.5);
    draw_window_decoration_icon(
        state,
        window,
        Ui2DecorationIconKind::ResizeHandle,
        icon_x,
        icon_y,
        icon_side,
    );
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
        let thumb_h = if window.kind == Ui2WindowKind::HostedBrowser {
            let snapshot = browser_surface_state_for_window(window);
            let viewport_h = snapshot.viewport_height.max(1) as f32;
            let content_h = snapshot.content_height.max(snapshot.viewport_height.max(1)) as f32;
            libm::fmaxf(10.0, (vbar.h * (viewport_h / content_h)).min(vbar.h))
        } else {
            libm::fminf(vbar.h, 18.0)
        };
        let thumb_y = if window.kind == Ui2WindowKind::HostedBrowser {
            let snapshot = browser_surface_state_for_window(window);
            let scroll_range = hosted_browser_scroll_max(&snapshot) as f32;
            let avail = (vbar.h - thumb_h).max(0.0);
            if scroll_range > 0.0 {
                vbar.y + (avail * ((normalized_hosted_browser_scroll(&snapshot) as f32) / scroll_range))
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
        let thumb_w = if window.kind == Ui2WindowKind::HostedBrowser {
            let snapshot = browser_surface_state_for_window(window);
            let viewport_w = snapshot.viewport_width.max(1) as f32;
            let content_w = snapshot.content_width.max(snapshot.viewport_width.max(1)) as f32;
            libm::fmaxf(10.0, (hbar.w * (viewport_w / content_w)).min(hbar.w))
        } else {
            libm::fminf((hbar.w - 2.0).max(8.0), 18.0)
        };
        let thumb_x = if window.kind == Ui2WindowKind::HostedBrowser {
            let snapshot = browser_surface_state_for_window(window);
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
    let frame_base_rgba = (0xD9, 0xDE, 0xE5, 0xFF);
    let frame_left_rgba = blend_rgba_over((0x00, 0x00, 0x00, 0x52), frame_base_rgba);
    let frame_mid_rgba = frame_base_rgba;
    let frame_right_rgba = blend_rgba_over((0xFF, 0xFF, 0xFF, 0x52), frame_base_rgba);
    let frame_left_rgba = modulate_rgba_alpha(frame_left_rgba, window.alpha);
    let frame_mid_rgba = modulate_rgba_alpha(frame_mid_rgba, window.alpha);
    let frame_right_rgba = modulate_rgba_alpha(frame_right_rgba, window.alpha);
    let title_rgba = modulate_rgba_alpha((0xF3, 0xF4, 0xF6, 0xFF), window.alpha);
    let body_rgba = (0xFB, 0xFB, 0xF8, window.alpha);
    let selection_rgba = window
        .selected_cursor_slots
        .first()
        .map(|slot_id| super::ui2_hid::cursor_color(*slot_id))
        .map(|rgba| modulate_rgba_alpha(rgba, window.alpha))
        .unwrap_or((0, 0, 0, 0));
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        body_rgba,
        state.view_w,
        state.view_h,
    );
    if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
        let title_mid_offset = browser_title_overlay_mid_offset(window, boot_probe_ms());
        let _ = crate::gfx::lyon::draw_horizontal_three_stop_rect_no_present(
            rect.x,
            rect.y,
            rect.w,
            if window.state == Ui2WindowStateKind::Minimized {
                rect.h
            } else {
                UI2_TITLE_H
            },
            frame_left_rgba,
            frame_mid_rgba,
            frame_right_rgba,
            title_mid_offset,
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
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x + 1.0,
            rect.y + 1.0,
            rect.w - 2.0,
            2.0,
            selection_rgba,
            state.view_w,
            state.view_h,
        );
    }

    if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
        if window.icon_id != 0 {
            let icon_side = 16.0f32;
            let icon_x = rect.x + 8.0;
            let icon_y = rect.y + ((UI2_TITLE_H - icon_side) * 0.5);
            draw_window_decoration_icon(
                state,
                window,
                Ui2DecorationIconKind::TitlebarWindow,
                icon_x,
                icon_y,
                icon_side,
            );
        }
        let title_face = crate::gfx::imbafont::ImbaFontFace::Loop;
        let title_text_h = UI2_TITLE_H.max(35.0);
        let title_tracking_px = -0.75f32;
        let title_w = crate::gfx::imbafont::measure_text_width_px_tracked(
            title_face,
            window.title.as_bytes(),
            title_text_h,
            title_tracking_px,
        );
        let title_left = if window.icon_id != 0 {
            rect.x + 28.0
        } else {
            rect.x + 8.0
        };
        let title_x = (rect.x + ((rect.w - title_w) * 0.5)).max(title_left);
        let title_y = rect.y + ((UI2_TITLE_H - title_text_h) * 0.5);
        if let Some(layout) = crate::gfx::imbafont::layout_text_top_left_tracked(
            title_face,
            window.title.as_bytes(),
            title_x,
            title_y,
            title_text_h,
            title_tracking_px,
        ) {
            let _ = crate::gfx::imbafont::draw_text_in_frame_negative(
                title_face,
                window.title.as_bytes(),
                &layout,
                state.view_w,
                state.view_h,
                title_rgba.3,
            );
        }
        draw_window_system_button(state, window, Ui2SystemButtonAction::Fork);
        draw_window_system_button(state, window, Ui2SystemButtonAction::Minimize);
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
            let w = (rect.w - left_inset - right_inset).max(1.0);
            let h = (rect.h - top_inset - bottom_inset).max(1.0);
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
    let h = (rect.h - top_inset - bottom_inset).max(1.0);
    if h <= 0.0 {
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
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.bottom_bar_visible {
        return None;
    }
    if window.state != Ui2WindowStateKind::Normal {
        return None;
    }
    let bar = window_bottom_bar_rect(state, window)?;
    let button_w = UI2_BOTTOM_RESIZE_BUTTON_W.min(bar.w.max(1.0));
    let button_h = UI2_BOTTOM_RESIZE_BUTTON_H.min(bar.h.max(1.0));
    Some(Ui2Rect::new(
        bar.x + bar.w - 1.0 - UI2_BOTTOM_RESIZE_BUTTON_PAD - button_w,
        bar.y + ((bar.h - button_h) * 0.5),
        button_w,
        button_h,
    ))
}

pub(super) fn window_system_button_rect(
    state: &Ui2State,
    window: &Ui2Window,
    action: Ui2SystemButtonAction,
) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.titlebar_visible {
        return None;
    }
    let rect = effective_window_rect(state, window);
    let button_y = rect.y + ((window_titlebar_height(window) - UI2_SYSTEM_BUTTON_H) * 0.5);
    let mut right_x = rect.x + rect.w - 6.0 - UI2_SYSTEM_BUTTON_W;
    let close_x = right_x;
    right_x -= UI2_SYSTEM_BUTTON_W + UI2_SYSTEM_BUTTON_GAP;
    let maximize_x = right_x;
    right_x -= UI2_SYSTEM_BUTTON_W + UI2_SYSTEM_BUTTON_GAP;
    let minimize_x = right_x;
    right_x -= UI2_SYSTEM_BUTTON_W + UI2_SYSTEM_BUTTON_GAP;
    let fork_x = right_x;
    let x = match action {
        Ui2SystemButtonAction::Fork => fork_x,
        Ui2SystemButtonAction::Minimize => minimize_x,
        Ui2SystemButtonAction::ToggleMaximize => maximize_x,
        Ui2SystemButtonAction::Close => close_x,
    };
    Some(Ui2Rect::new(
        x,
        button_y,
        UI2_SYSTEM_BUTTON_W,
        UI2_SYSTEM_BUTTON_H,
    ))
}

pub(super) fn system_button_action_at(
    state: &Ui2State,
    window_id: u32,
    x: f32,
    y: f32,
) -> Option<Ui2SystemButtonAction> {
    let window = state.windows.iter().find(|window| window.id == window_id)?;
    for action in [
        Ui2SystemButtonAction::Fork,
        Ui2SystemButtonAction::Minimize,
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
