use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use heapless::String as HString;

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::{Mutex, Once};

mod gadget;
mod ui2_browser;
mod ui2_font;
mod ui2_font_bucketproducer;
mod ui2_hid;
mod ui2_hit;
mod ui2_hosted;
mod ui2_win_btn;
mod ui2_win_deco;
mod ui2_win_register;

mod ui2_win;

use self::gadget::*;
use self::ui2_browser::*;
use self::ui2_font::*;
pub(crate) use self::ui2_font::{
    Ui2FontCpuAtlases, Ui2FontTextAlign, Ui2FontTier, Ui2FontVerticalAlign,
    ui2_font_blit_char_rgba, ui2_font_blit_text_rgba, ui2_font_char_advance_px,
    ui2_font_decode_cpu_atlases, ui2_font_has_glyph, ui2_font_measure_text,
    ui2_font_measure_text_for_px, ui2_font_native_line_height_px,
};
pub(crate) use self::ui2_font_bucketproducer::*;
use self::ui2_hid::*;
pub(crate) use self::ui2_hid::{
    cursor_color_rgba8_for_cursor_id, cursor_spirit_choices, set_cursor_spirit_glyph,
};
pub(crate) use self::ui2_hit::ui2_hit_task;
use self::ui2_hit::*;
pub(crate) use self::ui2_hosted::ui2_hosted_task;
use self::ui2_hosted::*;
pub use self::ui2_win::*;
use self::ui2_win_btn::*;
pub use self::ui2_win_deco::*;
use trueos_gfx_core::{
    RGB_VERTEX_SIZE, Rgba8, TEX_VERTEX_SIZE, TexVertex, ViewTransform, push_rgb_quad_px,
    push_tex_quad_px, push_tex_vertex_bytes,
};

const UI2_TWEMOJI_ICON_H: f32 = 64.0;
const UI2_SPRITE64_CELL_PX: f32 = 64.0;
const UI2_BAR_PAD_Y: f32 = 1.0;
const UI2_BAR_H: f32 = UI2_TWEMOJI_ICON_H + UI2_BAR_PAD_Y * 2.0;
const UI2_TITLE_H: f32 = UI2_BAR_H;
const UI2_BOTTOM_BAR_H: f32 = UI2_BAR_H;
const UI2_SYSTEM_VERTICAL_SCROLLBAR_W: f32 = UI2_TITLE_H * 0.5;
const UI2_SYSTEM_HORIZONTAL_SCROLLBAR_H: f32 = UI2_BOTTOM_BAR_H * 0.5;
const UI2_BROWSER_FORK_WINDOW_OFFSET_PX: f32 = 24.0;
const UI2_MINIMIZED_STRIP_W: f32 = 333.0;
const UI2_MINIMIZED_STRIP_GAP: f32 = 6.0;
const UI2_MINIMIZED_STRIP_PAD: f32 = 8.0;
const UI2_PRIMARY_BUTTON_MASK: u32 = 1;
const UI2_MIDDLE_BUTTON_MASK: u32 = 1 << 2;
const UI2_CLICK_SLOP_PX: f32 = 12.0;
const UI2_CURSOR_EVENT_BATCH: usize = 32;
const UI2_KEYBOARD_EVENT_BATCH: usize = 32;
const UI2_CURSOR_HIT_RADIUS_PX: f32 = 8.0;
const UI2_CURSOR_CAP: usize = 50;
const UI2_WINDOW_EDGE_TOUCH_PX: f32 = 1.0;
const UI2_WHEEL_SCROLL_STEP_PX: i32 = 16;
const UI2_INPUT_DIAG_LOG_FIRST: u32 = 8;
const UI2_INPUT_DIAG_LOG_EVERY: u32 = 32;
const UI2_COMPOSE_LOG_EVERY: u32 = 32;
const UI2_WINDOW_RESIZE_LEFT: u32 = 1 << 0;
const UI2_WINDOW_RESIZE_TOP: u32 = 1 << 1;
const UI2_WINDOW_RESIZE_RIGHT: u32 = 1 << 2;
const UI2_WINDOW_RESIZE_BOTTOM: u32 = 1 << 3;
const UI2_SCENE_TARGET_TEX_ID: u32 = 4_706;
const UI2_OVERLAY_TARGET_TEX_ID: u32 = 4_707;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Ui2Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Ui2Rect {
    #[inline]
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ui2CursorOverlayGlyphSpec {
    pub tex_id: u32,
    pub draw_w_px: u16,
    pub draw_h_px: u16,
    pub src_x: u16,
    pub src_y: u16,
    pub src_w: u16,
    pub src_h: u16,
    pub atlas_w: u16,
    pub atlas_h: u16,
}

const UI2_CURSOR_OVERLAY_TEX_ID_BASE: u32 = 49_100;
const UI2_CURSOR_OVERLAY_DIM_MIN_PX: u32 = 16;
const UI2_CURSOR_OVERLAY_DIM_MAX_PX: u32 = 64;
const UI2_CURSOR_OVERLAY_DIM_MIN_VIEW_H: u32 = 720;
const UI2_CURSOR_OVERLAY_DIM_MAX_VIEW_H: u32 = 2160;
const UI2_CURSOR_OVERLAY_CENTER_CUTOUT_RADIUS_PX: i32 = 2;
const ASYNC_TEX_STATUS_READY: i32 = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ui2CursorOverlayCacheEntry {
    tex_id: u32,
    glyph: char,
    ready_seq: u32,
    target_dim_px: u16,
    uploaded_w: u16,
    uploaded_h: u16,
}

impl Ui2CursorOverlayCacheEntry {
    const fn new() -> Self {
        Self {
            tex_id: 0,
            glyph: '\0',
            ready_seq: 0,
            target_dim_px: 0,
            uploaded_w: 0,
            uploaded_h: 0,
        }
    }
}

static UI2_CURSOR_OVERLAY_CACHE: Mutex<[Ui2CursorOverlayCacheEntry; 6]> = Mutex::new([
    Ui2CursorOverlayCacheEntry::new(),
    Ui2CursorOverlayCacheEntry::new(),
    Ui2CursorOverlayCacheEntry::new(),
    Ui2CursorOverlayCacheEntry::new(),
    Ui2CursorOverlayCacheEntry::new(),
    Ui2CursorOverlayCacheEntry::new(),
]);

#[inline]
fn cursor_overlay_cache_index(cursor_id: u32) -> usize {
    (cursor_id.saturating_sub(1) % 6) as usize
}

#[inline]
fn cursor_overlay_target_dim_px(view_h: u32) -> u16 {
    let clamped_h =
        view_h.clamp(UI2_CURSOR_OVERLAY_DIM_MIN_VIEW_H, UI2_CURSOR_OVERLAY_DIM_MAX_VIEW_H);
    let range_h = UI2_CURSOR_OVERLAY_DIM_MAX_VIEW_H - UI2_CURSOR_OVERLAY_DIM_MIN_VIEW_H;
    let range_dim = UI2_CURSOR_OVERLAY_DIM_MAX_PX - UI2_CURSOR_OVERLAY_DIM_MIN_PX;
    let scaled = if range_h == 0 {
        UI2_CURSOR_OVERLAY_DIM_MAX_PX
    } else {
        UI2_CURSOR_OVERLAY_DIM_MIN_PX
            + ((clamped_h - UI2_CURSOR_OVERLAY_DIM_MIN_VIEW_H) * range_dim + (range_h / 2))
                / range_h
    };
    scaled.clamp(UI2_CURSOR_OVERLAY_DIM_MIN_PX, UI2_CURSOR_OVERLAY_DIM_MAX_PX) as u16
}

#[inline]
fn cursor_overlay_texture_ready(tex_id: u32) -> bool {
    tex_id != 0
        && crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id) == ASYNC_TEX_STATUS_READY
}

fn cursor_overlay_build_sprite_rgba(
    glyph: &Ui2FontGlyph,
    target_dim_px: u16,
) -> Option<(u16, u16, Vec<u8>)> {
    let atlases = ui2_font_decode_cpu_atlases(glyph.tier.size_case())?;
    let atlas = ui2_font_cpu_atlas_for_glyph(&atlases, glyph)?;

    let dst_w = target_dim_px.max(1);
    let dst_h = target_dim_px.max(1);
    let mut rgba = alloc::vec![0u8; usize::from(dst_w) * usize::from(dst_h) * 4];
    let src_w = u32::from(glyph.region.src_w.max(1));
    let src_h = u32::from(glyph.region.src_h.max(1));
    let target_side = u32::from(target_dim_px.max(1));
    let draw_h = target_side;
    let draw_w = ((src_w * target_side) + (src_h / 2)) / src_h;
    let draw_w = draw_w.max(1).min(target_side);
    let off_x = ((target_side - draw_w) / 2) as usize;
    let off_y = ((target_side - draw_h) / 2) as usize;
    let atlas_w = atlas.width as usize;
    let src_x0 = glyph.region.src_x as usize;
    let src_y0 = glyph.region.src_y as usize;

    for y in 0..draw_h as usize {
        let sy = src_y0 + ((y * src_h as usize) / draw_h as usize).min(src_h as usize - 1);
        for x in 0..draw_w as usize {
            let sx = src_x0 + ((x * src_w as usize) / draw_w as usize).min(src_w as usize - 1);
            let src_idx = ((sy * atlas_w) + sx) * 4;
            let dst_x = off_x + x;
            let dst_y = off_y + y;
            let dst_idx = ((dst_y * usize::from(dst_w)) + dst_x) * 4;
            rgba[dst_idx..dst_idx + 4].copy_from_slice(&atlas.rgba[src_idx..src_idx + 4]);
        }
    }

    let center_x = i32::from(dst_w / 2);
    let center_y = i32::from(dst_h / 2);
    for y in 0..i32::from(dst_h) {
        for x in 0..i32::from(dst_w) {
            let dx = x - center_x;
            let dy = y - center_y;
            if (dx * dx) + (dy * dy)
                <= UI2_CURSOR_OVERLAY_CENTER_CUTOUT_RADIUS_PX
                    * UI2_CURSOR_OVERLAY_CENTER_CUTOUT_RADIUS_PX
            {
                let idx = ((y as usize * usize::from(dst_w)) + x as usize) * 4;
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 0;
            }
        }
    }

    Some((dst_w, dst_h, rgba))
}

pub(crate) fn cursor_overlay_glyph_spec(
    cursor_id: u32,
    slot_id: u32,
    view_h: u32,
) -> Option<Ui2CursorOverlayGlyphSpec> {
    let ch =
        cursor_spirit_glyph(slot_id).or_else(|| cursor_spirit_glyph_for_cursor_id(cursor_id))?;
    let glyph = ui2_font_resolve_glyph(Ui2FontTier::OneX, ch)?;
    if !glyph.ready {
        return None;
    }
    let cache_idx = cursor_overlay_cache_index(cursor_id);
    let target_dim_px = cursor_overlay_target_dim_px(view_h);
    let tex_id = UI2_CURSOR_OVERLAY_TEX_ID_BASE + cache_idx as u32;
    {
        let cache = UI2_CURSOR_OVERLAY_CACHE.lock();
        let entry = cache[cache_idx];
        if entry.tex_id == tex_id
            && entry.glyph == ch
            && entry.ready_seq == glyph.ready_seq
            && entry.target_dim_px == target_dim_px
            && entry.uploaded_w != 0
            && entry.uploaded_h != 0
        {
            if !cursor_overlay_texture_ready(tex_id) {
                return None;
            }
            return Some(Ui2CursorOverlayGlyphSpec {
                tex_id,
                draw_w_px: entry.uploaded_w,
                draw_h_px: entry.uploaded_h,
                src_x: 0,
                src_y: 0,
                src_w: entry.uploaded_w,
                src_h: entry.uploaded_h,
                atlas_w: entry.uploaded_w,
                atlas_h: entry.uploaded_h,
            });
        }
    }

    let (sprite_w, sprite_h, sprite_rgba) =
        cursor_overlay_build_sprite_rgba(&glyph, target_dim_px)?;
    if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        tex_id,
        u32::from(sprite_w),
        u32::from(sprite_h),
        sprite_rgba.as_slice(),
        0,
        "ui2-cursor-spirit",
    ) {
        return None;
    }

    let mut cache = UI2_CURSOR_OVERLAY_CACHE.lock();
    cache[cache_idx] = Ui2CursorOverlayCacheEntry {
        tex_id,
        glyph: ch,
        ready_seq: glyph.ready_seq,
        target_dim_px,
        uploaded_w: sprite_w,
        uploaded_h: sprite_h,
    };
    if !cursor_overlay_texture_ready(tex_id) {
        return None;
    }
    Some(Ui2CursorOverlayGlyphSpec {
        tex_id,
        draw_w_px: sprite_w,
        draw_h_px: sprite_h,
        src_x: 0,
        src_y: 0,
        src_w: sprite_w,
        src_h: sprite_h,
        atlas_w: sprite_w,
        atlas_h: sprite_h,
    })
}

#[inline]
fn blend_rgba_over(src: (u8, u8, u8, u8), dst: (u8, u8, u8, u8)) -> (u8, u8, u8, u8) {
    let sa = src.3 as u32;
    let inv = 255u32.saturating_sub(sa);
    let r = ((src.0 as u32 * sa) + (dst.0 as u32 * inv) + 127) / 255;
    let g = ((src.1 as u32 * sa) + (dst.1 as u32 * inv) + 127) / 255;
    let b = ((src.2 as u32 * sa) + (dst.2 as u32 * inv) + 127) / 255;
    let a = sa + ((dst.3 as u32 * inv) + 127) / 255;
    (r as u8, g as u8, b as u8, a.min(255) as u8)
}

#[inline]
fn modulate_alpha(alpha: u8, factor: u8) -> u8 {
    (((alpha as u32) * (factor as u32) + 127) / 255).min(255) as u8
}

#[inline]
fn modulate_rgba_alpha(rgba: (u8, u8, u8, u8), factor: u8) -> (u8, u8, u8, u8) {
    (rgba.0, rgba.1, rgba.2, modulate_alpha(rgba.3, factor))
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2CursorState {
    slot_id: u32,
    x: f32,
    y: f32,
    buttons_down: u32,
    press_x: f32,
    press_y: f32,
    press_window_id: u32,
    press_item_id: u32,
    press_armed: bool,
    selected_window_id: u32,
    hover_window_id: u32,
    hover_decoration_button: Option<Ui2DecorationHoverButton>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2SystemButtonAction {
    ToggleComposition,
    Fork,
    Minimize,
    Restore,
    ToggleMaximize,
    PreserveVm,
    RotateLeft,
    RotateRight,
    Close,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2DecorationHoverButton {
    System(Ui2SystemButtonAction),
    Resize,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowDecorationButton {
    ToggleComposition = 0,
    Fork = 1,
    Minimize = 2,
    Restore = 3,
    ToggleMaximize = 4,
    PreserveVm = 5,
    Close = 6,
}

impl Ui2WindowDecorationButton {
    pub const COUNT: usize = 7;
    pub const ALL_MASK: u32 = (1 << Self::COUNT) - 1;

    #[inline]
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::ToggleComposition),
            1 => Some(Self::Fork),
            2 => Some(Self::Minimize),
            3 => Some(Self::Restore),
            4 => Some(Self::ToggleMaximize),
            5 => Some(Self::PreserveVm),
            6 => Some(Self::Close),
            _ => None,
        }
    }

    #[inline]
    const fn from_action(action: Ui2SystemButtonAction) -> Option<Self> {
        match action {
            Ui2SystemButtonAction::ToggleComposition => Some(Self::ToggleComposition),
            Ui2SystemButtonAction::Fork => Some(Self::Fork),
            Ui2SystemButtonAction::Minimize => Some(Self::Minimize),
            Ui2SystemButtonAction::Restore => Some(Self::Restore),
            Ui2SystemButtonAction::ToggleMaximize => Some(Self::ToggleMaximize),
            Ui2SystemButtonAction::PreserveVm => Some(Self::PreserveVm),
            Ui2SystemButtonAction::Close => Some(Self::Close),
            Ui2SystemButtonAction::RotateLeft | Ui2SystemButtonAction::RotateRight => None,
        }
    }

    #[inline]
    const fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2WindowKind {
    HostedBrowser,
    HostedSurface,
    Hosted3d,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum Ui2WindowResizeMode {
    #[default]
    Auto = 0,
    Live = 1,
    PreviewCommit = 2,
}

impl Ui2WindowResizeMode {
    fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Auto),
            1 => Some(Self::Live),
            2 => Some(Self::PreviewCommit),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2WindowMoveDrag {
    active: bool,
    window_id: u32,
    cursor_slot_id: u32,
    grab_dx: f32,
    grab_dy: f32,
    edge_actions_armed: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2WindowEdgeDropAction {
    SnapLeft,
    SnapRight,
    Maximize,
    Minimize,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2WindowResizeDrag {
    active: bool,
    window_id: u32,
    cursor_slot_id: u32,
    live_apply: bool,
    edge_mask: u32,
    start_cursor_x: f32,
    start_cursor_y: f32,
    start_rect: Ui2Rect,
    preview_rect: Ui2Rect,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2WindowScrollDrag {
    active: bool,
    window_id: u32,
    cursor_slot_id: u32,
    horizontal: bool,
    track_rect: Ui2Rect,
    thumb_extent: f32,
    grab_offset: f32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2WindowScrollPanDrag {
    active: bool,
    window_id: u32,
    cursor_slot_id: u32,
    last_cursor_x: f32,
    last_cursor_y: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Ui2HostedSurfaceTile {
    pub tex_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub blend_enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ui2HostedInteractiveRect {
    pub item_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

type Ui2WindowTitle = HString<64>;

fn ui2_window_title_inline(title: &str) -> Ui2WindowTitle {
    let mut out = Ui2WindowTitle::new();
    let _ = out.push_str(title);
    out
}

#[derive(Clone)]
struct Ui2Window {
    id: u32,
    kind: Ui2WindowKind,
    spawn_task_index: Option<usize>,
    vm_origin_hint: bool,
    vm_origin_vm_id: u8,
    browser_instance_id: u32,
    hosted_browser_snapshot: UiHostedBrowserSnapshot,
    title: Ui2WindowTitle,
    icon_id: u32,
    title_icon_visible: bool,
    title_twemoji: char,
    title_icon_tex_id: u32,
    title_icon_load_seq: u32,
    title_icon_url: String,
    rect: Ui2Rect,
    restore_rect: Ui2Rect,
    z: i16,
    visible: bool,
    hit_test_visible: bool,
    composition_locked: bool,
    alpha: u8,
    decoration_mode: Ui2WindowDecorationMode,
    titlebar_visible: bool,
    bottom_bar_visible: bool,
    titlebar_button_visible_mask: u32,
    resize_button_visible: bool,
    rotate_buttons_visible: bool,
    content_rotation_quadrants: u8,
    left_scrollbar_visible: bool,
    bottom_scrollbar_visible: bool,
    resize_mode: Ui2WindowResizeMode,
    resize_maintain_aspect: bool,
    content_preserve_scale: bool,
    vertical_scrollbar_side: Ui2WindowVerticalScrollbarSide,
    horizontal_scrollbar_side: Ui2WindowHorizontalScrollbarSide,
    state: Ui2WindowStateKind,
    content_tex_id: u32,
    content_tex_blend: bool,
    hosted_surface_bg_rgba: [u8; 4],
    hosted_surface_fg_rgba: [u8; 4],
    hosted_surface_tiles: Vec<Ui2HostedSurfaceTile>,
    hosted_surface_interactives: Vec<Ui2HostedInteractiveRect>,
    last_clicked_item_id: u32,
    last_clicked_item_seq: u32,
    last_clicked_cursor_slot: u32,
    cursor_events: Vec<Ui2WindowCursorEvent>,
    container_sync_needed: bool,
    selected_cursor_slots: Vec<u32>,
    dirty: bool,
    content_present_dirty: bool,
    chrome_titlebar_dirty: bool,
    chrome_hover_clear_button: Option<Ui2DecorationHoverButton>,
    dirty_seq: u32,
    last_reason: &'static str,
    last_logged_dirty_seq: u32,
    last_logged_reason: &'static str,
}

#[inline]
fn ui2_system_button_count(window: &Ui2Window) -> usize {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.titlebar_visible {
        return 0;
    }
    let actions: &[Ui2SystemButtonAction] = match window.state {
        Ui2WindowStateKind::Minimized => &[
            Ui2SystemButtonAction::Close,
            Ui2SystemButtonAction::Restore,
            Ui2SystemButtonAction::ToggleMaximize,
        ],
        Ui2WindowStateKind::Maximized => &[
            Ui2SystemButtonAction::Close,
            Ui2SystemButtonAction::PreserveVm,
            Ui2SystemButtonAction::Minimize,
            Ui2SystemButtonAction::Fork,
            Ui2SystemButtonAction::ToggleComposition,
        ],
        Ui2WindowStateKind::Normal => &[
            Ui2SystemButtonAction::Close,
            Ui2SystemButtonAction::PreserveVm,
            Ui2SystemButtonAction::ToggleMaximize,
            Ui2SystemButtonAction::Minimize,
            Ui2SystemButtonAction::Fork,
            Ui2SystemButtonAction::ToggleComposition,
        ],
    };
    actions
        .iter()
        .filter(|action| {
            (**action != Ui2SystemButtonAction::PreserveVm || window.vm_origin_hint)
                && Ui2WindowDecorationButton::from_action(**action)
                    .map(|button| (window.titlebar_button_visible_mask & button.bit()) != 0)
                    .unwrap_or(true)
        })
        .count()
}

#[inline]
fn ui2_window_min_size(window: &Ui2Window) -> (f32, f32) {
    if window.decoration_mode != Ui2WindowDecorationMode::System {
        return (1.0, 1.0);
    }

    let mut min_w: f32 = 1.0;
    let mut min_h = if window.titlebar_visible {
        UI2_TITLE_H
    } else {
        0.0
    } + if window.bottom_bar_visible {
        UI2_BOTTOM_BAR_H
    } else {
        0.0
    };
    if window.left_scrollbar_visible {
        min_w += UI2_SYSTEM_VERTICAL_SCROLLBAR_W;
    }
    if window.bottom_scrollbar_visible {
        min_h += UI2_SYSTEM_HORIZONTAL_SCROLLBAR_H;
    }

    let button_count = ui2_system_button_count(window);
    if button_count != 0 {
        let s = UI2_TITLE_H;
        let gap = 1.0f32;
        let button_span = button_count as f32 * s + button_count.saturating_sub(1) as f32 * gap;
        min_w = min_w.max(button_span);
    }

    if window.bottom_bar_visible && window.state == Ui2WindowStateKind::Normal {
        if window.resize_button_visible {
            min_w = min_w.max(UI2_BOTTOM_BAR_H + 1.0);
        }
        if window.rotate_buttons_visible {
            let resize_slots = if window.resize_button_visible {
                1.0
            } else {
                0.0
            };
            min_w = min_w.max(UI2_BOTTOM_BAR_H * (2.0 + resize_slots) + 2.0);
        }
    }

    (min_w, min_h)
}

struct Ui2State {
    view_w: u32,
    view_h: u32,
    next_window_id: u32,
    compose_seq: u32,
    compose_reason: &'static str,
    last_logged_compose_seq: u32,
    last_logged_compose_reason: &'static str,
    last_logged_compose_dirty_count: usize,
    cursor_read_seq: u64,
    keyboard_read_seq: u64,
    non_physical_cursor_event_count: u32,
    synthetic_keyboard_event_count: u32,
    cursors: Vec<Ui2CursorState>,
    move_drags: Vec<Ui2WindowMoveDrag>,
    resize_drags: Vec<Ui2WindowResizeDrag>,
    scroll_drags: Vec<Ui2WindowScrollDrag>,
    scroll_pan_drags: Vec<Ui2WindowScrollPanDrag>,
    windows: Vec<Ui2Window>,
    last_athlas_small_ready_seq: u32,
    first_compose_signaled: bool,
    cursor_overlay_dirty: bool,
    chrome_overlay_dirty: bool,
    last_chrome_overlay_anim_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2ComposeWindowStats {
    visible_windows: usize,
    hosted_browser_windows: usize,
    hosted_browser_drawable: usize,
    hosted_browser_pending: usize,
    hosted_surface_windows: usize,
}

const UI2_WINDOW_TITLE_ICON_TEX_ID_BASE: u32 = 21_000;

#[derive(Clone, Debug)]
struct Ui2ComposeSurfaceTiming {
    id: u32,
    chrome_ms: u64,
    texture_ms: u64,
    placeholder_ms: u64,
    path: &'static str,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2WindowDrawTiming {
    chrome_ms: u64,
    texture_ms: u64,
    placeholder_ms: u64,
    content_path: &'static str,
}

static UI2_STATE: Once<Mutex<Ui2State>> = Once::new();
static UI2_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_DIRTY: AtomicBool = AtomicBool::new(false);
static UI2_BROWSER_WINDOW_ID: AtomicU32 = AtomicU32::new(0);
static UI2_CURSOR_SPRITE64_LOGS: AtomicU32 = AtomicU32::new(0);
static UI2_CHROME_GRADIENT_RECT_LOGS: AtomicU32 = AtomicU32::new(0);
static UI2_CHROME_SOLID_RECT_LOGS: AtomicU32 = AtomicU32::new(0);
static UI2_CONTENT_GPGPU_LOGS: AtomicU32 = AtomicU32::new(0);

const UI2_CHROME_OVERLAY_ANIM_FRAME_MS: u64 = 66;

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[inline]
fn elapsed_ms_since(started_at: Instant) -> u64 {
    Instant::now()
        .saturating_duration_since(started_at)
        .as_millis() as u64
}

#[inline]
fn ui2_now_ms() -> u64 {
    Instant::now().as_millis() as u64
}

fn init_state() -> &'static Mutex<Ui2State> {
    UI2_STATE.call_once(|| {
        let (view_w, view_h) = crate::intel::active_scanout_dimensions()
            .or_else(|| {
                crate::limine::framebuffer_response()
                    .and_then(|resp| resp.framebuffers().first().copied())
                    .map(|fb| (fb.width as u32, fb.height as u32))
            })
            .unwrap_or((1280, 800));

        let mut state = Ui2State {
            view_w,
            view_h,
            next_window_id: 1,
            compose_seq: 0,
            compose_reason: "boot",
            last_logged_compose_seq: 0,
            last_logged_compose_reason: "",
            last_logged_compose_dirty_count: 0,
            cursor_read_seq: 0,
            keyboard_read_seq: 0,
            non_physical_cursor_event_count: 0,
            synthetic_keyboard_event_count: 0,
            cursors: Vec::with_capacity(UI2_CURSOR_CAP),
            move_drags: Vec::new(),
            resize_drags: Vec::new(),
            scroll_drags: Vec::new(),
            scroll_pan_drags: Vec::new(),
            windows: Vec::new(),
            last_athlas_small_ready_seq: 0,
            first_compose_signaled: false,
            cursor_overlay_dirty: false,
            chrome_overlay_dirty: false,
            last_chrome_overlay_anim_ms: 0,
        };

        refresh_all_window_hit_entries(&mut state);

        Mutex::new(state)
    })
}

fn collect_compose_window_stats(state: &Ui2State) -> Ui2ComposeWindowStats {
    let mut stats = Ui2ComposeWindowStats::default();
    for window in &state.windows {
        if !window.visible {
            continue;
        }
        stats.visible_windows = stats.visible_windows.saturating_add(1);
        match window.kind {
            Ui2WindowKind::HostedBrowser => {
                stats.hosted_browser_windows = stats.hosted_browser_windows.saturating_add(1);
                if texture_is_drawable(window.content_tex_id) {
                    stats.hosted_browser_drawable = stats.hosted_browser_drawable.saturating_add(1);
                } else {
                    stats.hosted_browser_pending = stats.hosted_browser_pending.saturating_add(1);
                }
            }
            Ui2WindowKind::HostedSurface => {
                stats.hosted_surface_windows = stats.hosted_surface_windows.saturating_add(1);
            }
            Ui2WindowKind::Hosted3d => {
                stats.hosted_surface_windows = stats.hosted_surface_windows.saturating_add(1);
            }
        }
    }
    stats
}

#[inline]
fn upsert_move_drag(state: &mut Ui2State, drag: Ui2WindowMoveDrag) {
    if let Some(existing) = state
        .move_drags
        .iter_mut()
        .find(|existing| existing.cursor_slot_id == drag.cursor_slot_id)
    {
        *existing = drag;
    } else {
        state.move_drags.push(drag);
    }
}

#[inline]
fn upsert_resize_drag(state: &mut Ui2State, drag: Ui2WindowResizeDrag) {
    if let Some(existing) = state
        .resize_drags
        .iter_mut()
        .find(|existing| existing.cursor_slot_id == drag.cursor_slot_id)
    {
        *existing = drag;
    } else {
        state.resize_drags.push(drag);
    }
}

#[inline]
fn upsert_scroll_drag(state: &mut Ui2State, drag: Ui2WindowScrollDrag) {
    if let Some(existing) = state
        .scroll_drags
        .iter_mut()
        .find(|existing| existing.cursor_slot_id == drag.cursor_slot_id)
    {
        *existing = drag;
    } else {
        state.scroll_drags.push(drag);
    }
}

#[inline]
fn upsert_scroll_pan_drag(state: &mut Ui2State, drag: Ui2WindowScrollPanDrag) {
    if let Some(existing) = state
        .scroll_pan_drags
        .iter_mut()
        .find(|existing| existing.cursor_slot_id == drag.cursor_slot_id)
    {
        *existing = drag;
    } else {
        state.scroll_pan_drags.push(drag);
    }
}

#[inline]
fn clear_move_drag_for_slot(state: &mut Ui2State, slot_id: u32) {
    state
        .move_drags
        .retain(|drag| drag.cursor_slot_id != slot_id);
}

#[inline]
fn clear_resize_drag_for_slot(state: &mut Ui2State, slot_id: u32) {
    state
        .resize_drags
        .retain(|drag| drag.cursor_slot_id != slot_id);
}

#[inline]
fn clear_scroll_drag_for_slot(state: &mut Ui2State, slot_id: u32) {
    state
        .scroll_drags
        .retain(|drag| drag.cursor_slot_id != slot_id);
}

#[inline]
fn clear_scroll_pan_drag_for_slot(state: &mut Ui2State, slot_id: u32) {
    state
        .scroll_pan_drags
        .retain(|drag| drag.cursor_slot_id != slot_id);
}

#[inline]
fn clear_other_drag_modes_for_slot(state: &mut Ui2State, slot_id: u32) {
    clear_move_drag_for_slot(state, slot_id);
    clear_resize_drag_for_slot(state, slot_id);
    clear_scroll_drag_for_slot(state, slot_id);
    clear_scroll_pan_drag_for_slot(state, slot_id);
}

#[inline]
fn clear_window_drag_claims(state: &mut Ui2State, window_id: u32) {
    state.move_drags.retain(|drag| drag.window_id != window_id);
    state
        .resize_drags
        .retain(|drag| drag.window_id != window_id);
    state
        .scroll_drags
        .retain(|drag| drag.window_id != window_id);
    state
        .scroll_pan_drags
        .retain(|drag| drag.window_id != window_id);
}

fn sorted_window_indices(state: &Ui2State) -> Vec<usize> {
    let mut out: Vec<usize> = (0..state.windows.len()).collect();
    out.sort_by(|lhs, rhs| {
        let a = &state.windows[*lhs];
        let b = &state.windows[*rhs];
        match (a.state == Ui2WindowStateKind::Minimized, b.state == Ui2WindowStateKind::Minimized) {
            (true, false) => return CmpOrdering::Less,
            (false, true) => return CmpOrdering::Greater,
            _ => {}
        }
        match a.z.cmp(&b.z) {
            CmpOrdering::Equal => a.id.cmp(&b.id),
            other => other,
        }
    });
    out
}

fn window_mut(state: &mut Ui2State, id: u32) -> Option<&mut Ui2Window> {
    state.windows.iter_mut().find(|window| window.id == id)
}

fn rect_contains_point(rect: Ui2Rect, x: f32, y: f32) -> bool {
    x >= rect.x && y >= rect.y && x < (rect.x + rect.w) && y < (rect.y + rect.h)
}

fn window_browser_instance_id(window: &Ui2Window) -> u32 {
    if window.kind != Ui2WindowKind::HostedBrowser {
        return 0;
    }
    if window.browser_instance_id == 0 {
        PRIMARY_HOSTED_CONTENT_ID
    } else {
        window.browser_instance_id
    }
}

fn window_hosted_content_id(window: &Ui2Window) -> u32 {
    if window.browser_instance_id != 0 {
        return window.browser_instance_id;
    }
    if window.kind == Ui2WindowKind::HostedBrowser {
        PRIMARY_HOSTED_CONTENT_ID
    } else {
        0
    }
}

fn browser_surface_state_for_window(window: &Ui2Window) -> UiHostedSurfaceState {
    window.hosted_browser_snapshot.surface
}

fn hosted_surface_state_for_window(window: &Ui2Window) -> UiHostedSurfaceState {
    let content_id = window_hosted_content_id(window);
    if content_id == 0 {
        UiHostedSurfaceState::default()
    } else if window.kind == Ui2WindowKind::HostedBrowser {
        browser_surface_state_for_window(window)
    } else {
        hosted_surface_state(content_id)
    }
}

fn hosted_browser_scroll_max(snapshot: &UiHostedSurfaceState) -> u32 {
    let viewport_h = snapshot.viewport_height.max(1);
    let content_h = snapshot.content_height.max(viewport_h);
    content_h.saturating_sub(viewport_h)
}

fn hosted_browser_scroll_x_max(snapshot: &UiHostedSurfaceState) -> u32 {
    let viewport_w = snapshot.viewport_width.max(1);
    let content_w = snapshot.content_width.max(viewport_w);
    content_w.saturating_sub(viewport_w)
}

fn clamp_hosted_browser_scroll(snapshot: &UiHostedSurfaceState, requested_scroll: i64) -> u32 {
    let max_scroll = hosted_browser_scroll_max(snapshot) as i64;
    requested_scroll.clamp(0, max_scroll) as u32
}

fn clamp_hosted_browser_scroll_x(snapshot: &UiHostedSurfaceState, requested_scroll: i64) -> u32 {
    let max_scroll = hosted_browser_scroll_x_max(snapshot) as i64;
    requested_scroll.clamp(0, max_scroll) as u32
}

fn normalized_hosted_browser_scroll(snapshot: &UiHostedSurfaceState) -> u32 {
    clamp_hosted_browser_scroll(snapshot, snapshot.scroll_y as i64)
}

fn normalized_hosted_browser_scroll_x(snapshot: &UiHostedSurfaceState) -> u32 {
    clamp_hosted_browser_scroll_x(snapshot, snapshot.scroll_x as i64)
}

fn window_scroll_snapshot(window: &Ui2Window) -> Option<UiHostedSurfaceState> {
    match window.kind {
        Ui2WindowKind::HostedBrowser => Some(browser_surface_state_for_window(window)),
        Ui2WindowKind::HostedSurface => {
            let content_id = window_hosted_content_id(window);
            if content_id == 0 {
                None
            } else {
                Some(hosted_surface_state_for_window(window))
            }
        }
        Ui2WindowKind::Hosted3d => None,
    }
}

fn browser_interactive_state_for_window(window: &Ui2Window) -> UiHostedInteractiveState {
    window.hosted_browser_snapshot.interactive.clone()
}

fn refresh_hosted_browser_snapshot(window: &mut Ui2Window) {
    if window.kind != Ui2WindowKind::HostedBrowser {
        return;
    }
    window.hosted_browser_snapshot = hosted_browser_snapshot(window_browser_instance_id(window));
}

fn hosted_browser_interactive_seq(state: &Ui2State) -> u32 {
    state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| {
            let instance_id = window_browser_instance_id(window);
            let seq = hosted_interactive_seq(instance_id);
            instance_id.wrapping_mul(1315423911).wrapping_add(seq)
        })
        .fold(0u32, |acc, value| acc.wrapping_mul(16777619).wrapping_add(value))
}

fn apply_hosted_browser_dirty(state: &mut Ui2State, dirty: HostedBrowserDirtyMask) {
    if dirty.content == 0 && dirty.interactive == 0 {
        return;
    }

    let mut interactive_window_ids = Vec::new();

    for window in &mut state.windows {
        if window.kind != Ui2WindowKind::HostedBrowser {
            continue;
        }
        let instance_id = window_browser_instance_id(window);
        if !(1..=64).contains(&instance_id) {
            continue;
        }
        let bit = 1u64 << instance_id.saturating_sub(1);
        let content_dirty = (dirty.content & bit) != 0;
        let interactive_dirty = (dirty.interactive & bit) != 0;
        if content_dirty || interactive_dirty {
            refresh_hosted_browser_snapshot(window);
        }
        if content_dirty && !window.composition_locked {
            window.dirty = true;
            window.content_present_dirty = true;
            window.last_reason = "browser-content";
            UI2_DIRTY.store(true, Ordering::Release);
            state.compose_reason = "browser-content";
        }
        if interactive_dirty && !window.composition_locked {
            interactive_window_ids.push(window.id);
        }
    }

    if !interactive_window_ids.is_empty() {
        for window_id in interactive_window_ids {
            refresh_window_hit_entries(state, window_id);
        }
    }
}

fn window_is_renderable(window: &Ui2Window) -> bool {
    window.visible
}

fn window_content_participates_in_composition(window: &Ui2Window) -> bool {
    window.visible && !window.composition_locked
}

fn window_content_can_patch_primary(window: &Ui2Window) -> bool {
    !window.content_tex_blend
        && window.alpha == 255
        && window.content_rotation_quadrants == 0
        && window_content_participates_in_composition(window)
}

fn intel_direct_content_only_dirty(state: &Ui2State) -> bool {
    if !state.first_compose_signaled {
        return false;
    }
    if state.chrome_overlay_dirty
        && !state.windows.iter().any(|window| {
            window_is_renderable(window) && window_has_titlebar_chrome_overlay(state, window)
        })
    {
        return false;
    }

    let mut dirty_count = 0usize;
    for window in &state.windows {
        if !window.dirty {
            continue;
        }
        dirty_count = dirty_count.saturating_add(1);
        if !window.content_present_dirty
            || !window_is_renderable(window)
            || !window_content_can_patch_primary(window)
        {
            return false;
        }
    }

    dirty_count != 0
}

fn is_simple_click(press_x: f32, press_y: f32, release_x: f32, release_y: f32) -> bool {
    let dx = release_x - press_x;
    let dy = release_y - press_y;
    let slop_sq = UI2_CLICK_SLOP_PX * UI2_CLICK_SLOP_PX;
    (dx * dx) + (dy * dy) <= slop_sq
}

fn window_edge_drop_action(
    state: &Ui2State,
    cursor_x: f32,
    cursor_y: f32,
) -> Option<Ui2WindowEdgeDropAction> {
    let right_edge = (state.view_w.saturating_sub(1)) as f32;
    let bottom_edge = (state.view_h.saturating_sub(1)) as f32;
    let candidates = [
        (cursor_x.abs(), Ui2WindowEdgeDropAction::SnapLeft),
        ((right_edge - cursor_x).abs(), Ui2WindowEdgeDropAction::SnapRight),
        (cursor_y.abs(), Ui2WindowEdgeDropAction::Maximize),
        ((bottom_edge - cursor_y).abs(), Ui2WindowEdgeDropAction::Minimize),
    ];
    let mut best: Option<(f32, Ui2WindowEdgeDropAction)> = None;
    for candidate in candidates {
        if candidate.0 > UI2_WINDOW_EDGE_TOUCH_PX {
            continue;
        }
        if best
            .as_ref()
            .map(|current| candidate.0 < current.0)
            .unwrap_or(true)
        {
            best = Some(candidate);
        }
    }
    best.map(|(_, action)| action)
}

fn apply_window_edge_drop_action(
    state: &mut Ui2State,
    id: u32,
    action: Ui2WindowEdgeDropAction,
) -> bool {
    match action {
        Ui2WindowEdgeDropAction::SnapLeft => {
            set_window_rect_in_state(state, id, left_half_window_rect(state), "window-snap-left")
        }
        Ui2WindowEdgeDropAction::SnapRight => {
            set_window_rect_in_state(state, id, right_half_window_rect(state), "window-snap-right")
        }
        Ui2WindowEdgeDropAction::Maximize => maximize_window_in_state(state, id),
        Ui2WindowEdgeDropAction::Minimize => minimize_window_in_state(state, id),
    }
}

#[inline]
fn is_valid_resize_edge_mask(edge_mask: u32) -> bool {
    if edge_mask == 0 {
        return false;
    }
    if (edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 && (edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
        return false;
    }
    if (edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 && (edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
        return false;
    }
    true
}

fn draw_resize_preview_outline(state: &Ui2State) {
    let outer = (0x2B, 0x6C, 0xD6, 0xFF);
    let inner = (0xD9, 0xE7, 0xFF, 0xFF);
    for drag in &state.resize_drags {
        if !drag.active || drag.live_apply {
            continue;
        }
        let rect = drag.preview_rect;
        if !(rect.w > 0.0 && rect.h > 0.0) {
            continue;
        }
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x,
            rect.y,
            rect.w,
            1.0,
            outer,
            state.view_w,
            state.view_h,
        );
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x,
            rect.y + rect.h - 1.0,
            rect.w,
            1.0,
            outer,
            state.view_w,
            state.view_h,
        );
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x,
            rect.y,
            1.0,
            rect.h,
            outer,
            state.view_w,
            state.view_h,
        );
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            rect.x + rect.w - 1.0,
            rect.y,
            1.0,
            rect.h,
            outer,
            state.view_w,
            state.view_h,
        );
        if rect.w > 4.0 && rect.h > 4.0 {
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                rect.x + 1.0,
                rect.y + 1.0,
                rect.w - 2.0,
                1.0,
                inner,
                state.view_w,
                state.view_h,
            );
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                rect.x + 1.0,
                rect.y + rect.h - 2.0,
                rect.w - 2.0,
                1.0,
                inner,
                state.view_w,
                state.view_h,
            );
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                rect.x + 1.0,
                rect.y + 1.0,
                1.0,
                rect.h - 2.0,
                inner,
                state.view_w,
                state.view_h,
            );
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                rect.x + rect.w - 2.0,
                rect.y + 1.0,
                1.0,
                rect.h - 2.0,
                inner,
                state.view_w,
                state.view_h,
            );
        }
    }
}

#[inline]
fn cursor_overlay_center_px(nx: f32, ny: f32, vp_w: u32, vp_h: u32) -> (f32, f32) {
    let max_x = vp_w.saturating_sub(1).max(1) as f32;
    let max_y = vp_h.saturating_sub(1).max(1) as f32;
    (nx * max_x, ny * max_y)
}

fn draw_cursor_cross_overlay(state: &Ui2State, center_x: f32, center_y: f32, color: Rgba8) {
    let half_span = (state.view_h as f32 * 0.013).clamp(12.0, 24.0);
    let half_thickness = (half_span * 0.22).clamp(2.0, 6.0);
    let _ = draw_rgb_rect_no_present(
        center_x - half_span,
        center_y - half_thickness,
        half_span * 2.0,
        half_thickness * 2.0,
        color,
        state.view_w,
        state.view_h,
    );
    let _ = draw_rgb_rect_no_present(
        center_x - half_thickness,
        center_y - half_span,
        half_thickness * 2.0,
        half_span * 2.0,
        color,
        state.view_w,
        state.view_h,
    );
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2ChromeSpriteScope {
    None,
    All,
    Active,
    ActiveTitlebar,
}

fn window_has_active_chrome_overlay(state: &Ui2State, window: &Ui2Window) -> bool {
    !window.selected_cursor_slots.is_empty()
        || state.cursors.iter().any(|cursor| {
            cursor.hover_window_id == window.id && cursor.hover_decoration_button.is_some()
        })
}

fn window_has_titlebar_chrome_overlay(state: &Ui2State, window: &Ui2Window) -> bool {
    window.chrome_titlebar_dirty
        || window.chrome_hover_clear_button.is_some()
        || window_has_active_chrome_overlay(state, window)
}

fn draw_cursor_overlay_layer_intel_sprite64(
    state: &Ui2State,
    chrome_scope: Ui2ChromeSpriteScope,
    present: bool,
) -> bool {
    if !crate::gfx::is_intel_active() {
        return false;
    }

    let mut placements = Vec::new();
    let mut chrome_count = 0usize;
    let mut dock_count = 0usize;
    if chrome_scope != Ui2ChromeSpriteScope::None {
        for idx in sorted_window_indices(state) {
            let window = &state.windows[idx];
            if !window_is_renderable(window) {
                continue;
            }
            if chrome_scope == Ui2ChromeSpriteScope::Active
                && !window_has_active_chrome_overlay(state, window)
            {
                continue;
            }
            if chrome_scope == Ui2ChromeSpriteScope::ActiveTitlebar {
                if !window_has_titlebar_chrome_overlay(state, window) {
                    continue;
                }
                chrome_count = chrome_count.saturating_add(
                    collect_window_titlebar_chrome_sprite64_placements(
                        state,
                        window,
                        effective_window_rect(state, window),
                        &mut placements,
                        true,
                    ),
                );
            } else {
                chrome_count =
                    chrome_count.saturating_add(collect_window_chrome_sprite64_placements(
                        state,
                        window,
                        effective_window_rect(state, window),
                        &mut placements,
                    ));
            }
        }
        if chrome_scope == Ui2ChromeSpriteScope::All {
            dock_count =
                ui2_win_register::collect_offline_dock_sprite64_placements(state, &mut placements);
        }
    }

    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slots();
    if cursors.is_empty() && placements.is_empty() {
        return true;
    }

    let cursor_count = cursors.len();
    for (idx, (slot_id, cx, cy)) in cursors.into_iter().enumerate() {
        let cursor_id = (idx as u32).saturating_add(1);
        let Some(ch) =
            cursor_spirit_glyph(slot_id).or_else(|| cursor_spirit_glyph_for_cursor_id(cursor_id))
        else {
            return false;
        };
        let Some(region) = crate::gfx::althlasfont::twemoji::twemoji_lookup_glyph_region(ch) else {
            return false;
        };

        let nx = if cx.is_finite() {
            cx.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let ny = if cy.is_finite() {
            cy.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let (center_x, center_y) = cursor_overlay_center_px(nx, ny, state.view_w, state.view_h);
        placements.push(crate::intel::gpgpu::GpgpuTwemojiSprite64Placement {
            slot: region.slot,
            dst_x: libm::roundf(center_x - 32.0) as i32,
            dst_y: libm::roundf(center_y - 32.0) as i32,
        });
    }

    let Some(result) = crate::intel::gpgpu::twemoji_sprite64_worklist_primary(&placements, present)
    else {
        return false;
    };
    let ok = result.ok && result.submitted;
    let log_n = UI2_CURSOR_SPRITE64_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_n < 12 || !ok {
        crate::log!(
            "ui2: surroundings sprite64-worklist ok={} chrome={} dock={} cursors={} desc={} walkers={} submit_ms={} present={} present_ms={} last_slot={} last_dst={},{}\n",
            ok as u8,
            chrome_count,
            dock_count,
            cursor_count,
            result.descriptors,
            result.walkers,
            result.submit_ms,
            result.presented as u8,
            result.present_ms,
            result.last_slot,
            result.last_dst_xy.x,
            result.last_dst_xy.y
        );
    }
    ok
}

fn draw_chrome_gradient_rects_intel_gpgpu(state: &Ui2State) -> bool {
    if !crate::gfx::is_intel_active() {
        return false;
    }
    if !intel_ui2_chrome_rect_path_enabled() {
        let log_n = UI2_CHROME_GRADIENT_RECT_LOGS.fetch_add(1, Ordering::Relaxed);
        if log_n < 8 {
            crate::log!("ui2: chrome gradients-gpgpu disabled reason=chrome-rect-path-disabled\n");
        }
        let _ = state;
        return false;
    }

    let mut rects = Vec::new();
    let mut window_count = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window_is_renderable(window) {
            continue;
        }
        let added = collect_window_chrome_gradient_rects(
            state,
            window,
            effective_window_rect(state, window),
            &mut rects,
        );
        if added > 0 {
            window_count = window_count.saturating_add(1);
        }
    }
    let dock_rects = ui2_win_register::collect_offline_dock_gradient_rects(state, &mut rects);
    if rects.is_empty() {
        return true;
    }

    let Some(result) = crate::intel::gpgpu::gradient_rects_rgba8_over_primary(&rects, false) else {
        return false;
    };
    let ok = result.ok;
    let log_n = UI2_CHROME_GRADIENT_RECT_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_n < 16 || !ok {
        let mode = if result.blend_descs == 0 {
            "direct-primary"
        } else {
            "alpha-over-primary"
        };
        crate::log!(
            "ui2: chrome gradients-gpgpu ok={} mode={} windows={} dock_rects={} rects={} artifacts=gradient_rect_worklist_rgba8,alpha_blend_worklist_rgba8 gradient_descs={} gradient_walkers={} gradient_submits={} gradient_ms={} blend_descs={} blend_walkers={} blend_submits={} blend_ms={} present={} present_ms={} total_ms={}\n",
            ok as u8,
            mode,
            window_count,
            dock_rects,
            result.rects,
            result.fill_descs,
            result.fill_walkers,
            result.fill_submits,
            result.fill_ms,
            result.blend_descs,
            result.blend_walkers,
            result.blend_submits,
            result.blend_ms,
            result.presented as u8,
            result.present_ms,
            result.total_ms
        );
    }
    ok
}

fn draw_active_chrome_overlay_intel_gpgpu(state: &mut Ui2State) -> bool {
    if !crate::gfx::is_intel_active()
        || !state.first_compose_signaled
        || !intel_ui2_chrome_rect_path_enabled()
    {
        return false;
    }

    let mut rects = Vec::new();
    let mut window_count = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window_is_renderable(window) || !window_has_titlebar_chrome_overlay(state, window) {
            continue;
        }
        let added = collect_window_titlebar_chrome_gradient_rects(
            state,
            window,
            effective_window_rect(state, window),
            &mut rects,
        );
        if added > 0 {
            window_count = window_count.saturating_add(1);
        }
    }

    if rects.is_empty() {
        let ok = draw_cursor_overlay_layer_intel_sprite64(
            state,
            Ui2ChromeSpriteScope::ActiveTitlebar,
            true,
        );
        state.chrome_overlay_dirty = !ok;
        if ok {
            state.cursor_overlay_dirty = false;
            for window in &mut state.windows {
                window.chrome_titlebar_dirty = false;
                window.chrome_hover_clear_button = None;
            }
        }
        return ok;
    }

    let Some(result) = crate::intel::gpgpu::gradient_rects_rgba8_over_primary(&rects, false) else {
        return false;
    };
    let sprites_ok =
        draw_cursor_overlay_layer_intel_sprite64(state, Ui2ChromeSpriteScope::ActiveTitlebar, true);
    let ok = result.ok && sprites_ok;
    state.chrome_overlay_dirty = !ok;
    if ok {
        state.cursor_overlay_dirty = false;
        for window in &mut state.windows {
            window.chrome_titlebar_dirty = false;
            window.chrome_hover_clear_button = None;
        }
    }
    let log_n = UI2_CHROME_GRADIENT_RECT_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_n < 16 || !ok {
        crate::log!(
            "ui2: active titlebar chrome overlay-gpgpu ok={} windows={} rects={} gradient_ms={} sprites_present={} total_ms={}\n",
            ok as u8,
            window_count,
            result.rects,
            result.fill_ms.saturating_add(result.blend_ms),
            sprites_ok as u8,
            result.total_ms
        );
    }
    ok
}

fn active_chrome_overlay_due(state: &Ui2State, now_ms: u64) -> bool {
    if state.cursor_overlay_dirty {
        return true;
    }
    if state.chrome_overlay_dirty {
        return true;
    }
    if state
        .windows
        .iter()
        .any(|window| window_is_renderable(window) && window.chrome_titlebar_dirty)
    {
        return true;
    }
    state.first_compose_signaled
        && crate::gfx::is_intel_active()
        && now_ms.saturating_sub(state.last_chrome_overlay_anim_ms)
            >= UI2_CHROME_OVERLAY_ANIM_FRAME_MS
        && state
            .windows
            .iter()
            .any(|window| window_is_renderable(window) && !window.selected_cursor_slots.is_empty())
}

fn draw_chrome_solid_rects_intel_gpgpu(state: &Ui2State, skip_bands: bool) -> bool {
    if !crate::gfx::is_intel_active() {
        return false;
    }
    if !intel_ui2_chrome_rect_path_enabled() {
        let log_n = UI2_CHROME_SOLID_RECT_LOGS.fetch_add(1, Ordering::Relaxed);
        if log_n < 8 {
            crate::log!(
                "ui2: chrome solid-rects-gpgpu disabled reason=span-shaped-artifacts-regress-frame-pacing\n"
            );
        }
        let _ = state;
        return false;
    }

    let mut rects = Vec::new();
    let mut window_count = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window_is_renderable(window) {
            continue;
        }
        let added = collect_window_chrome_solid_rects(
            state,
            window,
            effective_window_rect(state, window),
            &mut rects,
            skip_bands,
        );
        if added > 0 {
            window_count = window_count.saturating_add(1);
        }
    }
    let dock_rects = if skip_bands {
        0
    } else {
        ui2_win_register::collect_offline_dock_solid_rects(state, &mut rects)
    };
    if rects.is_empty() {
        return true;
    }

    let Some(result) = crate::intel::gpgpu::solid_rects_rgba8_over_primary(&rects, false) else {
        return false;
    };
    let ok = result.ok;
    let log_n = UI2_CHROME_SOLID_RECT_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_n < 16 || !ok {
        let mode = if result.blend_descs == 0 {
            "direct-primary"
        } else {
            "alpha-over-primary"
        };
        crate::log!(
            "ui2: chrome solid-rects-gpgpu ok={} mode={} windows={} dock_rects={} rects={} artifacts=fill_rect_worklist_rgba8,alpha_blend_worklist_rgba8 fill_descs={} fill_walkers={} fill_submits={} fill_ms={} blend_descs={} blend_walkers={} blend_submits={} blend_ms={} present={} present_ms={} total_ms={}\n",
            ok as u8,
            mode,
            window_count,
            dock_rects,
            result.rects,
            result.fill_descs,
            result.fill_walkers,
            result.fill_submits,
            result.fill_ms,
            result.blend_descs,
            result.blend_walkers,
            result.blend_submits,
            result.blend_ms,
            result.presented as u8,
            result.present_ms,
            result.total_ms
        );
    }
    ok
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2IntelContentGpgpuStats {
    candidates: usize,
    submitted: usize,
    skipped: usize,
    spans: usize,
    submits: usize,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
    pixels: usize,
}

fn clip_texture_content_to_primary(
    state: &Ui2State,
    content: Ui2Rect,
    draw_x: f32,
    draw_y: f32,
    draw_w: f32,
    draw_h: f32,
    tex_w: u32,
    tex_h: u32,
) -> Option<(crate::intel::gpgpu::GpgpuRect, crate::intel::gpgpu::GpgpuPoint)> {
    if tex_w == 0 || tex_h == 0 || !(draw_w > 0.0 && draw_h > 0.0) {
        return None;
    }

    let clip_x0 = libm::floorf(draw_x.max(content.x).max(0.0));
    let clip_y0 = libm::floorf(draw_y.max(content.y).max(0.0));
    let clip_x1 = libm::ceilf(
        (draw_x + draw_w)
            .min(content.x + content.w)
            .min(state.view_w as f32),
    );
    let clip_y1 = libm::ceilf(
        (draw_y + draw_h)
            .min(content.y + content.h)
            .min(state.view_h as f32),
    );
    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
        return None;
    }

    let src_x = (clip_x0 - draw_x).max(0.0) as u32;
    let src_y = (clip_y0 - draw_y).max(0.0) as u32;
    if src_x >= tex_w || src_y >= tex_h {
        return None;
    }

    let mut width = (clip_x1 - clip_x0) as u32;
    let mut height = (clip_y1 - clip_y0) as u32;
    width = width.min(tex_w.saturating_sub(src_x));
    height = height.min(tex_h.saturating_sub(src_y));
    if width == 0 || height == 0 {
        return None;
    }

    Some((
        crate::intel::gpgpu::GpgpuRect::new(src_x as i32, src_y as i32, width, height),
        crate::intel::gpgpu::GpgpuPoint::new(clip_x0 as i32, clip_y0 as i32),
    ))
}

fn draw_window_content_textures_intel_gpgpu(state: &Ui2State, dirty_only: bool) -> bool {
    if !crate::gfx::is_intel_active() {
        return false;
    }

    let mut stats = Ui2IntelContentGpgpuStats::default();
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if dirty_only && !window.content_present_dirty {
            continue;
        }
        if !window_is_renderable(window)
            || !window_content_participates_in_composition(window)
            || window.state == Ui2WindowStateKind::Minimized
            || window.content_tex_id == 0
        {
            continue;
        }
        let Some(content) = window_content_rect(state, window) else {
            continue;
        };
        if !(content.w > 0.0 && content.h > 0.0) || !texture_is_drawable(window.content_tex_id) {
            continue;
        }
        stats.candidates = stats.candidates.saturating_add(1);

        let Some((tex_w, tex_h)) = texture_dimensions(window.content_tex_id) else {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        };
        let Some(src_surface) =
            crate::r::io::cabi::texture_gpgpu_rgba8_surface(window.content_tex_id)
        else {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        };

        let (draw_x, draw_y, draw_w, draw_h) = if window.content_preserve_scale {
            let draw_w = tex_w as f32;
            let draw_h = tex_h as f32;
            (
                content.x + (content.w - draw_w) * 0.5,
                content.y + (content.h - draw_h) * 0.5,
                draw_w,
                draw_h,
            )
        } else {
            (content.x, content.y, content.w, content.h)
        };

        let Some((src_rect, dst_xy)) = clip_texture_content_to_primary(
            state, content, draw_x, draw_y, draw_w, draw_h, tex_w, tex_h,
        ) else {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        };

        let mut flags = if window.content_tex_blend || window.alpha < 255 {
            crate::intel::gpgpu::COMPOSITE_WORKLIST_FLAG_SRC_OVER
        } else {
            crate::intel::gpgpu::COMPOSITE_WORKLIST_FLAG_COPY
        };
        let color_rgba = if window.alpha < 255 {
            flags |= crate::intel::gpgpu::COMPOSITE_WORKLIST_FLAG_TINT_ALPHA;
            ((window.alpha as u32) << 24) | 0x00FF_FFFF
        } else {
            crate::intel::gpgpu::COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA
        };

        let Some(submit_stats) =
            crate::intel::gpgpu::alpha_blend_rgba8_tiled_over_primary_with_flags_stats(
                src_surface,
                src_rect,
                dst_xy,
                64,
                16,
                flags,
                color_rgba,
            )
        else {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        };
        stats.submitted = stats.submitted.saturating_add(1);
        stats.spans = stats.spans.saturating_add(submit_stats.spans);
        stats.submits = stats.submits.saturating_add(submit_stats.submits);
        stats.submit_ms = stats.submit_ms.saturating_add(submit_stats.submit_ms);
        stats.present_ms = stats.present_ms.saturating_add(submit_stats.present_ms);
        stats.total_ms = stats.total_ms.saturating_add(submit_stats.total_ms);
        stats.pixels = stats
            .pixels
            .saturating_add((src_rect.width as usize).saturating_mul(src_rect.height as usize));
    }

    let ok = stats.skipped == 0;
    let log_n = UI2_CONTENT_GPGPU_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_n < 24 || !ok {
        crate::log!(
            "ui2: content-gpgpu ok={} candidates={} submitted={} skipped={} spans={} submits={} submit_ms={} present_ms={} total_ms={} pixels={} artifact=alpha_blend_worklist_rgba8 path={}\n",
            ok as u8,
            stats.candidates,
            stats.submitted,
            stats.skipped,
            stats.spans,
            stats.submits,
            stats.submit_ms,
            stats.present_ms,
            stats.total_ms,
            stats.pixels,
            if dirty_only {
                "dirty-texture-to-primary"
            } else {
                "texture-to-primary"
            }
        );
    }
    ok
}

fn intel_ui2_chrome_rect_path_enabled() -> bool {
    true
}

fn draw_cursor_overlay_layer(state: &Ui2State) {
    if draw_cursor_overlay_layer_intel_sprite64(state, Ui2ChromeSpriteScope::None, false) {
        return;
    }

    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slots();
    for (idx, (slot_id, cx, cy)) in cursors.into_iter().enumerate() {
        let cursor_id = (idx as u32).saturating_add(1);
        let nx = if cx.is_finite() {
            cx.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let ny = if cy.is_finite() {
            cy.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let (center_x, center_y) = cursor_overlay_center_px(nx, ny, state.view_w, state.view_h);
        let Some(glyph) = cursor_overlay_glyph_spec(cursor_id, slot_id, state.view_h) else {
            draw_cursor_cross_overlay(
                state,
                center_x,
                center_y,
                cursor_color_rgba8_for_cursor_id(cursor_id),
            );
            continue;
        };

        let draw_w = f32::from(glyph.draw_w_px.max(1));
        let draw_h = f32::from(glyph.draw_h_px.max(1));
        let atlas_w = f32::from(glyph.atlas_w.max(1));
        let atlas_h = f32::from(glyph.atlas_h.max(1));
        let u0 = f32::from(glyph.src_x) / atlas_w;
        let v0 = f32::from(glyph.src_y) / atlas_h;
        let u1 = f32::from(glyph.src_x.saturating_add(glyph.src_w)) / atlas_w;
        let v1 = f32::from(glyph.src_y.saturating_add(glyph.src_h)) / atlas_h;
        let _ = draw_texture_rect_uv_rgba_no_present(
            glyph.tex_id,
            center_x - draw_w * 0.5,
            center_y - draw_h * 0.5,
            draw_w,
            draw_h,
            u0,
            v0,
            u1,
            v1,
            state.view_w,
            state.view_h,
            true,
            (255, 255, 255, 255),
        );
    }
}

fn note_window_dirty(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.dirty = true;
    window.content_present_dirty = false;
    window.last_reason = reason;
    UI2_DIRTY.store(true, Ordering::Release);
    true
}

fn note_window_content_present_dirty(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    let was_dirty = window.dirty;
    window.dirty = true;
    if !was_dirty {
        window.content_present_dirty = true;
    }
    window.last_reason = reason;
    UI2_DIRTY.store(true, Ordering::Release);
    true
}

fn note_cursor_overlay_dirty(state: &mut Ui2State, reason: &'static str) {
    state.cursor_overlay_dirty = true;
    state.compose_reason = reason;
    if !crate::gfx::is_intel_active() || !state.first_compose_signaled {
        UI2_DIRTY.store(true, Ordering::Release);
    }
}

fn note_window_titlebar_chrome_dirty(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let force_full = !crate::gfx::is_intel_active() || !state.first_compose_signaled;
    {
        let Some(window) = window_mut(state, id) else {
            return false;
        };
        window.chrome_titlebar_dirty = true;
        window.last_reason = reason;
        if force_full {
            window.dirty = true;
            window.content_present_dirty = false;
        }
    }
    state.chrome_overlay_dirty = true;
    state.compose_reason = reason;
    if force_full {
        UI2_DIRTY.store(true, Ordering::Release);
    }
    true
}

fn note_window_chrome_hover_clear_dirty(
    state: &mut Ui2State,
    id: u32,
    button: Ui2DecorationHoverButton,
    reason: &'static str,
) -> bool {
    let force_full = !crate::gfx::is_intel_active() || !state.first_compose_signaled;
    {
        let Some(window) = window_mut(state, id) else {
            return false;
        };
        window.chrome_titlebar_dirty = true;
        window.chrome_hover_clear_button = Some(button);
        window.last_reason = reason;
        if force_full {
            window.dirty = true;
            window.content_present_dirty = false;
        }
    }
    state.chrome_overlay_dirty = true;
    state.compose_reason = reason;
    if force_full {
        UI2_DIRTY.store(true, Ordering::Release);
    }
    true
}

fn note_window_viewport_sync_needed(state: &mut Ui2State, id: u32) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.container_sync_needed = true;
    queue_hosted_container_sync();
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_signal_hosted_browser_dirty(content_id: u32, flags: u32) {
    crate::surfer::signal_hosted_browser_dirty(content_id, flags);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_create(
    title_ptr: *const u8,
    title_len: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
) -> u32 {
    if title_ptr.is_null() {
        return 0;
    }
    let title = core::slice::from_raw_parts(title_ptr, title_len);
    let Ok(title) = core::str::from_utf8(title) else {
        return 0;
    };
    let rect = Ui2Rect {
        x: x as f32,
        y: y as f32,
        w: width.max(1) as f32,
        h: height.max(1) as f32,
    };
    let id = create_window(
        title,
        rect,
        z.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        alpha.min(255) as u8,
    );
    if id != 0 {
        let _ = set_window_current_vm_origin(id);
    }
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_app_window_create(
    title_ptr: *const u8,
    title_len: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
) -> u32 {
    if title_ptr.is_null() {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: plain create rejected null title ptr"
        ));
        return 0;
    }
    let title = core::slice::from_raw_parts(title_ptr, title_len);
    let Ok(title) = core::str::from_utf8(title) else {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: plain create rejected invalid utf8 title len={}",
            title_len
        ));
        return 0;
    };
    let title = String::from(title);
    if crate::hv::current_guest_execution_context_vm_id().is_some() {
        return crate::hv::defer_blueprint_app_window_create(
            "plain",
            title.as_str(),
            x,
            y,
            width,
            height,
            z,
            alpha,
            0,
            false,
        );
    }
    let rect = Ui2Rect {
        x: x as f32,
        y: y as f32,
        w: width.max(1) as f32,
        h: height.max(1) as f32,
    };
    let id = create_window(
        title.as_str(),
        rect,
        z.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        alpha.min(255) as u8,
    );
    if id != 0 {
        let _ = set_window_current_vm_origin(id);
        let _ = focus_window(id);
        crate::hv::register_blueprint_app_window(id, "plain", title.as_str());
    } else {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: plain create failed title={} rect=({},{} {}x{}) z={} alpha={}",
            title.as_str(),
            x,
            y,
            width,
            height,
            z,
            alpha
        ));
    }
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_surface_window_create(
    title_ptr: *const u8,
    title_len: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
    tex_id: u32,
    blend_enabled: u32,
) -> u32 {
    if title_ptr.is_null() || tex_id == 0 {
        return 0;
    }
    let title = core::slice::from_raw_parts(title_ptr, title_len);
    let Ok(title) = core::str::from_utf8(title) else {
        return 0;
    };
    let title = String::from(title);
    if crate::hv::current_guest_execution_context_vm_id().is_some() {
        return crate::hv::defer_blueprint_app_window_create(
            "surface",
            title.as_str(),
            x,
            y,
            width,
            height,
            z,
            alpha,
            tex_id,
            blend_enabled != 0,
        );
    }
    let content_rect = Ui2Rect {
        x: x as f32,
        y: y as f32,
        w: width.max(1) as f32,
        h: height.max(1) as f32,
    };
    let id = create_hosted_surface_content_window(
        title.as_str(),
        content_rect,
        z.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        alpha.min(255) as u8,
        tex_id,
        blend_enabled != 0,
    );
    if id != 0 {
        let _ = set_window_current_vm_origin(id);
    }
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_app_surface_window_create(
    title_ptr: *const u8,
    title_len: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
    tex_id: u32,
    blend_enabled: u32,
) -> u32 {
    if title_ptr.is_null() || tex_id == 0 {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: surface create rejected title_ptr_null={} tex_id={}",
            title_ptr.is_null(),
            tex_id
        ));
        return 0;
    }
    let title = core::slice::from_raw_parts(title_ptr, title_len);
    let Ok(title) = core::str::from_utf8(title) else {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: surface create rejected invalid utf8 title len={} tex_id={}",
            title_len, tex_id
        ));
        return 0;
    };
    let title = String::from(title);
    if crate::hv::current_guest_execution_context_vm_id().is_some() {
        return crate::hv::defer_blueprint_app_window_create(
            "surface",
            title.as_str(),
            x,
            y,
            width,
            height,
            z,
            alpha,
            tex_id,
            blend_enabled != 0,
        );
    }
    let content_rect = Ui2Rect {
        x: x as f32,
        y: y as f32,
        w: width.max(1) as f32,
        h: height.max(1) as f32,
    };
    let id = Ui2SurfaceWindow::new(
        title.as_str(),
        content_rect,
        z.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        alpha.min(255) as u8,
        tex_id,
        blend_enabled != 0,
        [0x08, 0x0C, 0x12, 0xFF],
    )
    .map(|surface| surface.window_id())
    .unwrap_or(0);
    if id != 0 {
        let _ = set_window_current_vm_origin(id);
        let _ = focus_window(id);
        crate::hv::register_blueprint_app_window(id, "surface", title.as_str());
    } else {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: surface create failed title={} tex={} rect=({},{} {}x{}) z={} alpha={} blend={}",
            title.as_str(),
            tex_id,
            x,
            y,
            width,
            height,
            z,
            alpha,
            blend_enabled
        ));
    }
    id
}

#[inline]
fn vm_deferred_window_ok(window_id: u32, op: &'static str) -> Option<i32> {
    let Some(owner_vm_id) = crate::hv::deferred_blueprint_app_window_vm_id(window_id) else {
        return None;
    };
    let Some(current_vm_id) = crate::hv::deferred_blueprint_app_window_current_vm(window_id) else {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: deferred window op={} window={} owner_vm={} status=rejected-vm-mismatch",
            op, window_id, owner_vm_id
        ));
        return Some(-1);
    };
    let ok = crate::hv::note_deferred_blueprint_app_window_op(window_id, op);
    if op != "request-repaint" {
        crate::hv::log_blueprint_app_window_event(format_args!(
            "app-window-broker: vm{} deferred window op={} window={} status=queued",
            current_vm_id, op, window_id
        ));
    }
    if op == "begin-move" {
        return Some(if ok { 0 } else { -1 });
    }
    Some(0)
}

#[inline]
fn vm_deferred_window_any(window_id: u32) -> bool {
    crate::hv::deferred_blueprint_app_window_vm_id(window_id).is_some()
}

#[inline]
fn ui2_cabi_target_window_id(window_id: u32) -> u32 {
    if crate::hv::current_guest_execution_context_vm_id().is_some() {
        return window_id;
    }
    crate::hv::host_blueprint_app_window_id(window_id)
}

#[inline]
fn current_vm_origin_id() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
}

#[inline]
fn set_window_current_vm_origin(id: u32) -> bool {
    if let Some(vm_id) = current_vm_origin_id() {
        set_window_vm_origin(id, Some(vm_id))
    } else {
        set_window_vm_origin_hint(id, true)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_info(
    window_id: u32,
    out_info: *mut TrueosUi2WindowInfo,
) -> i32 {
    if out_info.is_null() {
        return -1;
    }
    let window_id = ui2_cabi_target_window_id(window_id);
    if vm_deferred_window_any(window_id) {
        let Some(info) = crate::hv::deferred_blueprint_app_window_info_current_vm(window_id) else {
            return -1;
        };
        *out_info = info;
        return 0;
    }
    let Some(info) = window_info_by_id(window_id) else {
        return -1;
    };
    *out_info = info;
    0
}

fn copy_window_cursor_events_to_payload(
    events: Vec<Ui2WindowCursorEvent>,
    out_cap: u32,
    payload: &mut [u8],
) -> (usize, usize) {
    let event_size = core::mem::size_of::<v::vcabi::TrueosUi2WindowCursorEvent>();
    let max_events = core::cmp::min(out_cap as usize, payload.len() / event_size);
    let wrote = core::cmp::min(events.len(), max_events);
    for (idx, event) in events.into_iter().take(wrote).enumerate() {
        let raw = v::vcabi::TrueosUi2WindowCursorEvent {
            slot_id: event.slot_id,
            buttons_down: event.buttons_down,
            flags: event.flags,
            wheel: event.wheel,
            reserved0: 0,
            x: event.x,
            y: event.y,
        };
        let dst = idx * event_size;
        unsafe {
            core::ptr::copy_nonoverlapping(
                (&raw as *const v::vcabi::TrueosUi2WindowCursorEvent) as *const u8,
                payload[dst..dst + event_size].as_mut_ptr(),
                event_size,
            );
        }
    }
    (wrote, wrote * event_size)
}

pub fn host_ui2_window_cursor_events(
    window_id: u32,
    out_cap: u32,
    payload: &mut [u8],
) -> (usize, usize) {
    if let Some(vm_id) = crate::hv::deferred_blueprint_app_window_vm_id(window_id) {
        let _ = crate::hv::materialize_deferred_blueprint_app_windows(vm_id);
    }
    let host_window_id = crate::hv::host_blueprint_app_window_id(window_id);
    let events = take_window_cursor_events(host_window_id);
    copy_window_cursor_events_to_payload(events, out_cap, payload)
}

fn guest_ui2_window_take_cursor_events(
    window_id: u32,
    out: *mut v::vcabi::TrueosUi2WindowCursorEvent,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return 0;
    }
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, wrote) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI2_WINDOW_CURSOR_EVENTS,
        window_id as u64,
        out_cap as u64,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return 0;
    }
    let event_size = core::mem::size_of::<v::vcabi::TrueosUi2WindowCursorEvent>();
    let got = core::cmp::min(wrote as usize, out_cap as usize);
    let bytes_len = got.saturating_mul(event_size);
    if got == 0 || out.is_null() || bytes_len > payload.len() {
        return got as u32;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(payload.as_ptr(), out as *mut u8, bytes_len);
    }
    got as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_take_cursor_events(
    window_id: u32,
    out: *mut v::vcabi::TrueosUi2WindowCursorEvent,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_ui2_window_take_cursor_events(window_id, out, out_cap);
    }
    if out.is_null() {
        return 0;
    }
    let window_id = ui2_cabi_target_window_id(window_id);
    let events = take_window_cursor_events(window_id);
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (wrote, bytes_len) = copy_window_cursor_events_to_payload(events, out_cap, &mut payload);
    unsafe {
        core::ptr::copy_nonoverlapping(payload.as_ptr(), out as *mut u8, bytes_len);
    }
    wrote as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_title(
    window_id: u32,
    title_ptr: *const u8,
    title_len: usize,
) -> i32 {
    if title_ptr.is_null() {
        return -1;
    }
    let title = core::slice::from_raw_parts(title_ptr, title_len);
    let Ok(title) = core::str::from_utf8(title) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-title") {
        let _ = crate::hv::note_deferred_blueprint_app_window_title(window_id, title);
        return rc;
    }
    if set_window_title(window_id, title) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_request_repaint(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "request-repaint") {
        return rc;
    }
    if request_window_repaint(window_id, "portal-window-repaint") {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_icon(window_id: u32, icon_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-icon") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(window_id, "set-icon", icon_id);
        return rc;
    }
    if set_window_icon(window_id, icon_id) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_position(
    window_id: u32,
    x: i32,
    y: i32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-position") {
        let _ = crate::hv::note_deferred_blueprint_app_window_position(window_id, x, y);
        return rc;
    }
    if move_window(window_id, x as f32, y as f32) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_size(
    window_id: u32,
    width: u32,
    height: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-size") {
        let _ = crate::hv::note_deferred_blueprint_app_window_size(window_id, width, height);
        return rc;
    }
    if resize_window_content(window_id, width as f32, height as f32) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_decorations(window_id: u32, mode: u32) -> i32 {
    let Some(mode) = Ui2WindowDecorationMode::from_u32(mode) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-decorations") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(
            window_id,
            "set-decorations",
            mode as u32,
        );
        return rc;
    }
    if set_window_decorations(window_id, mode) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_titlebar_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-titlebar-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-titlebar-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_titlebar_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_bottom_bar_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-bottom-bar-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-bottom-bar-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_bottom_bar_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_title_icon_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-title-icon-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-title-icon-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_title_icon_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_decoration_button_visible(
    window_id: u32,
    button: u32,
    visible: u32,
) -> i32 {
    let Some(button) = Ui2WindowDecorationButton::from_u32(button) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-decoration-button-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_button_visible(
            window_id,
            button as u32,
            visible != 0,
        );
        return rc;
    }
    if set_window_titlebar_button_visible(window_id, button, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_resize_button_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-resize-button-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-resize-button-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_resize_button_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_hit_test_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-hit-test-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-hit-test-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_hit_test_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_vertical_scrollbar_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-vertical-scrollbar-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-vertical-scrollbar-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_left_scrollbar_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_horizontal_scrollbar_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-horizontal-scrollbar-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-horizontal-scrollbar-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_bottom_scrollbar_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_resize_maintain_aspect(
    window_id: u32,
    maintain_aspect: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-resize-maintain-aspect") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-resize-maintain-aspect",
            maintain_aspect != 0,
        );
        return rc;
    }
    if set_window_resize_maintain_aspect(window_id, maintain_aspect != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_content_preserve_scale(
    window_id: u32,
    preserve_scale: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-content-preserve-scale") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-content-preserve-scale",
            preserve_scale != 0,
        );
        return rc;
    }
    if set_window_content_preserve_scale(window_id, preserve_scale != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_resize_mode(window_id: u32, mode: u32) -> i32 {
    let Some(mode) = Ui2WindowResizeMode::from_u32(mode) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-resize-mode") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(
            window_id,
            "set-resize-mode",
            mode as u32,
        );
        return rc;
    }
    if set_window_resize_mode(window_id, mode) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_vertical_scrollbar_side(
    window_id: u32,
    side: u32,
) -> i32 {
    let Some(side) = Ui2WindowVerticalScrollbarSide::from_u32(side) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-vertical-scrollbar-side") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(
            window_id,
            "set-vertical-scrollbar-side",
            side as u32,
        );
        return rc;
    }
    if set_window_vertical_scrollbar_side(window_id, side) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_horizontal_scrollbar_side(
    window_id: u32,
    side: u32,
) -> i32 {
    let Some(side) = Ui2WindowHorizontalScrollbarSide::from_u32(side) else {
        return -1;
    };
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-horizontal-scrollbar-side") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(
            window_id,
            "set-horizontal-scrollbar-side",
            side as u32,
        );
        return rc;
    }
    if set_window_horizontal_scrollbar_side(window_id, side) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_rotate_buttons_visible(
    window_id: u32,
    visible: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-rotate-buttons-visible") {
        let _ = crate::hv::note_deferred_blueprint_app_window_bool(
            window_id,
            "set-rotate-buttons-visible",
            visible != 0,
        );
        return rc;
    }
    if set_window_rotate_buttons_visible(window_id, visible != 0) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_content_rotation_quadrants(
    window_id: u32,
    quadrants: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "set-content-rotation") {
        let _ = crate::hv::note_deferred_blueprint_app_window_u32(
            window_id,
            "set-content-rotation",
            quadrants % 4,
        );
        return rc;
    }
    if set_window_content_rotation_quadrants(window_id, (quadrants % 4) as u8) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_minimize(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "minimize") {
        return rc;
    }
    if minimize_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_maximize(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "maximize") {
        return rc;
    }
    if maximize_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_restore(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "restore") {
        return rc;
    }
    if restore_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_focus(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "focus") {
        return rc;
    }
    if focus_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_close(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "close") {
        return rc;
    }
    if close_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_begin_move(window_id: u32) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "begin-move") {
        return rc;
    }
    if begin_window_move(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_begin_resize(
    window_id: u32,
    edge_mask: u32,
) -> i32 {
    let window_id = ui2_cabi_target_window_id(window_id);
    if let Some(rc) = vm_deferred_window_ok(window_id, "begin-resize") {
        return rc;
    }
    if begin_window_resize(window_id, edge_mask) {
        0
    } else {
        -1
    }
}

#[inline]
fn round_to_u32(v: f32, min: u32) -> u32 {
    let rounded = libm::roundf(v.max(min as f32));
    if rounded.is_finite() && rounded > 0.0 {
        rounded as u32
    } else {
        min
    }
}

fn with_window_content_scissor<T>(state: &Ui2State, content: Ui2Rect, f: impl FnOnce() -> T) -> T {
    let x0 = libm::floorf(content.x.max(0.0));
    let y0 = libm::floorf(content.y.max(0.0));
    let x1 = libm::ceilf((content.x + content.w).min(state.view_w as f32));
    let y1 = libm::ceilf((content.y + content.h).min(state.view_h as f32));

    let width = (x1 - x0).max(0.0) as u32;
    let height = (y1 - y0).max(0.0) as u32;
    if width == 0 || height == 0 {
        return f();
    }

    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_scissor(x0 as u32, y0 as u32, width, height)
    };
    let out = f();
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_clear_scissor() };
    out
}

#[inline]
fn rects_intersect(a: Ui2Rect, b: Ui2Rect) -> bool {
    a.w > 0.0
        && a.h > 0.0
        && b.w > 0.0
        && b.h > 0.0
        && a.x < b.x + b.w
        && a.x + a.w > b.x
        && a.y < b.y + b.h
        && a.y + a.h > b.y
}

#[inline]
fn union_rect(a: Ui2Rect, b: Ui2Rect) -> Ui2Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Ui2Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

fn clamp_rect_to_view(rect: Ui2Rect, view_w: u32, view_h: u32) -> Option<Ui2Rect> {
    let x0 = libm::floorf(rect.x.max(0.0));
    let y0 = libm::floorf(rect.y.max(0.0));
    let x1 = libm::ceilf((rect.x + rect.w).min(view_w as f32));
    let y1 = libm::ceilf((rect.y + rect.h).min(view_h as f32));
    let w = (x1 - x0).max(0.0);
    let h = (y1 - y0).max(0.0);
    if w <= 0.0 || h <= 0.0 {
        None
    } else {
        Some(Ui2Rect::new(x0, y0, w, h))
    }
}

fn content_present_dirty_scissor(state: &Ui2State, present_to_screen: bool) -> Option<Ui2Rect> {
    if !present_to_screen || !state.first_compose_signaled {
        return None;
    }
    if state.cursor_overlay_dirty {
        return None;
    }

    let mut dirty_count = 0usize;
    let mut rect = None;
    for window in &state.windows {
        if !window.dirty {
            continue;
        }
        dirty_count = dirty_count.saturating_add(1);
        if !window.content_present_dirty || !window_is_renderable(window) {
            return None;
        }
        let window_rect = effective_window_rect(state, window);
        rect = Some(match rect {
            Some(existing) => union_rect(existing, window_rect),
            None => window_rect,
        });
    }

    if dirty_count == 0 {
        return None;
    }
    clamp_rect_to_view(rect?, state.view_w, state.view_h)
}

fn draw_texture_rect_uv_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    view_w: u32,
    view_h: u32,
    blend_enabled: bool,
    alpha: u8,
) -> bool {
    draw_texture_rect_uv_rgba_no_present(
        tex_id,
        x,
        y,
        width,
        height,
        u0,
        v0,
        u1,
        v1,
        view_w,
        view_h,
        blend_enabled,
        (255, 255, 255, alpha),
    )
}

fn draw_texture_rect_uv_rgba_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    view_w: u32,
    view_h: u32,
    blend_enabled: bool,
    rgba: (u8, u8, u8, u8),
) -> bool {
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let transform = ViewTransform::from_extent(view_w, view_h);
    let mut verts = alloc::vec::Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    push_tex_quad_px(
        &mut verts,
        transform,
        x,
        y,
        x + width,
        y + height,
        [u0, v0, u1, v1],
        Rgba8::new(rgba.0, rgba.1, rgba.2, rgba.3),
    );
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(
            if blend_enabled || rgba.3 < 255 { 1 } else { 0 },
            0x0302,
            0x0303,
            0x0302,
            0x0303,
            0,
            0,
        )
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

fn draw_texture_rect_uv_rotated_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    rotation_quadrants: u8,
    view_w: u32,
    view_h: u32,
    blend_enabled: bool,
    alpha: u8,
) -> bool {
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }
    let color = Rgba8::new(255, 255, 255, alpha);
    let transform = ViewTransform::from_extent(view_w, view_h);
    let left = x;
    let right = x + width;
    let top = y;
    let bottom = y + height;
    let (uv_tl, uv_tr, uv_br, uv_bl) = match rotation_quadrants % 4 {
        1 => ([u0, v1], [u0, v0], [u1, v0], [u1, v1]),
        2 => ([u1, v1], [u0, v1], [u0, v0], [u1, v0]),
        3 => ([u1, v0], [u1, v1], [u0, v1], [u0, v0]),
        _ => ([u0, v0], [u1, v0], [u1, v1], [u0, v1]),
    };
    let mut verts = alloc::vec::Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    for (px, py, uv) in [
        (left, bottom, uv_bl),
        (right, bottom, uv_br),
        (right, top, uv_tr),
        (left, bottom, uv_bl),
        (right, top, uv_tr),
        (left, top, uv_tl),
    ] {
        let vertex = transform.tex_vertex_px(px, py, uv[0], uv[1], color);
        push_tex_vertex_bytes(
            &mut verts,
            TexVertex {
                x: vertex.x,
                y: vertex.y,
                u: vertex.u,
                v: vertex.v,
                color,
            },
        );
    }
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(
            if blend_enabled || alpha < 255 { 1 } else { 0 },
            0x0302,
            0x0303,
            0x0302,
            0x0303,
            0,
            0,
        )
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

fn draw_texture_rect_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    view_w: u32,
    view_h: u32,
    blend_enabled: bool,
    alpha: u8,
) -> bool {
    draw_texture_rect_uv_no_present(
        tex_id,
        x,
        y,
        width,
        height,
        0.0,
        0.0,
        1.0,
        1.0,
        view_w,
        view_h,
        blend_enabled,
        alpha,
    )
}

fn draw_rgb_rect_no_present(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    rgba: Rgba8,
    view_w: u32,
    view_h: u32,
) -> bool {
    if !(width > 0.0 && height > 0.0) {
        return false;
    }

    let transform = ViewTransform::from_extent(view_w, view_h);
    let mut verts = alloc::vec::Vec::with_capacity(6 * RGB_VERTEX_SIZE);
    push_rgb_quad_px(&mut verts, transform, x, y, x + width, y + height, rgba);
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
            verts.as_ptr(),
            verts.len(),
        )
    };
    rc == 0
}

fn draw_hosted_surface_tiles(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
    snapshot: Option<UiHostedSurfaceState>,
) -> bool {
    let viewport_w = snapshot
        .map(|it| it.viewport_width.max(1))
        .unwrap_or_else(|| round_to_u32(content.w, 1));
    let viewport_h = snapshot
        .map(|it| it.viewport_height.max(1))
        .unwrap_or_else(|| round_to_u32(content.h, 1));
    let scroll_x = snapshot
        .as_ref()
        .map(normalized_hosted_browser_scroll_x)
        .unwrap_or(0);
    let scroll_y = snapshot
        .as_ref()
        .map(normalized_hosted_browser_scroll)
        .unwrap_or(0);
    let vis_x0 = i64::from(scroll_x);
    let vis_y0 = i64::from(scroll_y);
    let vis_x1 = vis_x0.saturating_add(i64::from(viewport_w));
    let vis_y1 = vis_y0.saturating_add(i64::from(viewport_h));
    let scale_x = content.w / viewport_w.max(1) as f32;
    let scale_y = content.h / viewport_h.max(1) as f32;
    let mut drew_any = false;

    let bg = window.hosted_surface_bg_rgba;
    if bg[3] != 0 {
        drew_any |= draw_rgb_rect_no_present(
            content.x,
            content.y,
            content.w,
            content.h,
            Rgba8::new(bg[0], bg[1], bg[2], modulate_alpha(bg[3], window.alpha)),
            state.view_w,
            state.view_h,
        );
    }

    for tile in &window.hosted_surface_tiles {
        if tile.tex_id == 0 || tile.width == 0 || tile.height == 0 {
            continue;
        }
        if !texture_is_drawable(tile.tex_id) {
            continue;
        }

        let tile_x0 = i64::from(tile.x);
        let tile_y0 = i64::from(tile.y);
        let tile_x1 = tile_x0.saturating_add(i64::from(tile.width));
        let tile_y1 = tile_y0.saturating_add(i64::from(tile.height));
        let clip_x0 = tile_x0.max(vis_x0);
        let clip_y0 = tile_y0.max(vis_y0);
        let clip_x1 = tile_x1.min(vis_x1);
        let clip_y1 = tile_y1.min(vis_y1);
        if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
            continue;
        }

        let clipped_w = (clip_x1 - clip_x0) as f32;
        let clipped_h = (clip_y1 - clip_y0) as f32;
        let draw_x = content.x + (clip_x0 - vis_x0) as f32 * scale_x;
        let draw_y = content.y + (clip_y0 - vis_y0) as f32 * scale_y;
        let draw_w = clipped_w * scale_x;
        let draw_h = clipped_h * scale_y;
        let u0 = (clip_x0 - tile_x0) as f32 / tile.width as f32;
        let v0 = (clip_y0 - tile_y0) as f32 / tile.height as f32;
        let u1 = (clip_x1 - tile_x0) as f32 / tile.width as f32;
        let v1 = (clip_y1 - tile_y0) as f32 / tile.height as f32;
        let fg = window.hosted_surface_fg_rgba;
        drew_any |= draw_texture_rect_uv_rgba_no_present(
            tile.tex_id,
            draw_x,
            draw_y,
            draw_w,
            draw_h,
            u0,
            v0,
            u1,
            v1,
            state.view_w,
            state.view_h,
            tile.blend_enabled,
            (fg[0], fg[1], fg[2], modulate_alpha(fg[3], window.alpha)),
        );
    }

    drew_any
}

#[inline]
fn texture_is_drawable(tex_id: u32) -> bool {
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    tex_id != 0
        && crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id) == ASYNC_TEX_STATUS_READY
}

#[inline]
fn window_title_icon_tex_id(window_id: u32) -> u32 {
    UI2_WINDOW_TITLE_ICON_TEX_ID_BASE.saturating_add(window_id.saturating_sub(1))
}

fn draw_window_content_placeholder(
    _state: &Ui2State,
    _window: &Ui2Window,
    _content: Ui2Rect,
    _headline: &[u8],
    _subline: &[u8],
) {
}

#[inline]
fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    if tex_id == 0 {
        return None;
    }
    let mut width = 0u32;
    let mut height = 0u32;
    let ok = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_texture_dimensions(
            tex_id,
            &mut width as *mut u32,
            &mut height as *mut u32,
        ) == 0
    };
    if ok && width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

#[inline]
fn draw_window_texture_content(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
    tex_id: u32,
) -> bool {
    if window.kind == Ui2WindowKind::HostedSurface {
        if let Some(snapshot) = window_scroll_snapshot(window) {
            let viewport_w = snapshot.viewport_width.max(1);
            let content_w = snapshot.content_width.max(viewport_w);
            if content_w > viewport_w && window.content_rotation_quadrants % 4 == 0 {
                let Some((_tex_w, _tex_h)) = texture_dimensions(tex_id) else {
                    return false;
                };
                let scroll_x = normalized_hosted_browser_scroll_x(&snapshot).min(content_w);
                let visible_w = viewport_w.min(content_w.saturating_sub(scroll_x)).max(1);
                let u0 = scroll_x as f32 / content_w as f32;
                let u1 = scroll_x.saturating_add(visible_w) as f32 / content_w as f32;
                let draw_w = content.w * (visible_w as f32 / viewport_w as f32);
                return draw_texture_rect_uv_no_present(
                    tex_id,
                    content.x,
                    content.y,
                    draw_w,
                    content.h,
                    u0.clamp(0.0, 1.0),
                    0.0,
                    u1.clamp(0.0, 1.0),
                    1.0,
                    state.view_w,
                    state.view_h,
                    window.content_tex_blend,
                    window.alpha,
                );
            }
        }
    }

    let rotation = window.content_rotation_quadrants % 4;
    if !window.content_preserve_scale {
        if rotation != 0 {
            return draw_texture_rect_uv_rotated_no_present(
                tex_id,
                content.x,
                content.y,
                content.w,
                content.h,
                0.0,
                0.0,
                1.0,
                1.0,
                rotation,
                state.view_w,
                state.view_h,
                window.content_tex_blend,
                window.alpha,
            );
        }
        return draw_texture_rect_no_present(
            tex_id,
            content.x,
            content.y,
            content.w,
            content.h,
            state.view_w,
            state.view_h,
            window.content_tex_blend,
            window.alpha,
        );
    }

    let Some((tex_w, tex_h)) = texture_dimensions(tex_id) else {
        return false;
    };
    let draw_w = tex_w as f32;
    let draw_h = tex_h as f32;
    let draw_x = content.x + (content.w - draw_w) * 0.5;
    let draw_y = content.y + (content.h - draw_h) * 0.5;
    let visible_x0 = libm::fmaxf(draw_x, content.x);
    let visible_y0 = libm::fmaxf(draw_y, content.y);
    let visible_x1 = libm::fminf(draw_x + draw_w, content.x + content.w);
    let visible_y1 = libm::fminf(draw_y + draw_h, content.y + content.h);
    let visible_w = (visible_x1 - visible_x0).max(0.0);
    let visible_h = (visible_y1 - visible_y0).max(0.0);
    if !(visible_w > 0.0 && visible_h > 0.0) {
        return false;
    }

    let u0 = (visible_x0 - draw_x) / draw_w;
    let v0 = (visible_y0 - draw_y) / draw_h;
    let u1 = (visible_x1 - draw_x) / draw_w;
    let v1 = (visible_y1 - draw_y) / draw_h;

    if rotation != 0 {
        draw_texture_rect_uv_rotated_no_present(
            tex_id,
            visible_x0,
            visible_y0,
            visible_w,
            visible_h,
            u0,
            v0,
            u1,
            v1,
            rotation,
            state.view_w,
            state.view_h,
            window.content_tex_blend,
            window.alpha,
        )
    } else {
        draw_texture_rect_uv_no_present(
            tex_id,
            visible_x0,
            visible_y0,
            visible_w,
            visible_h,
            u0,
            v0,
            u1,
            v1,
            state.view_w,
            state.view_h,
            window.content_tex_blend,
            window.alpha,
        )
    }
}

fn draw_window_frame(
    state: &Ui2State,
    window: &Ui2Window,
    skip_twemoji_sprite64_chrome: bool,
    skip_lyon_rects: bool,
    force_content_locked: bool,
) -> Ui2WindowDrawTiming {
    if !window_is_renderable(window) {
        return Ui2WindowDrawTiming::default();
    }

    let chrome_started_at = Instant::now();
    let rect = effective_window_rect(state, window);
    let content_rect = window_content_rect(state, window);
    draw_window_chrome(state, window, rect, skip_twemoji_sprite64_chrome, skip_lyon_rects);

    let chrome_ms = elapsed_ms_since(chrome_started_at);

    if force_content_locked || !window_content_participates_in_composition(window) {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: 0,
            placeholder_ms: 0,
            content_path: if force_content_locked {
                "intel-chrome-baseline"
            } else {
                "locked"
            },
        };
    }

    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            if let Some(content) = content_rect {
                return draw_hosted_browser_window_content(state, window, content, chrome_ms);
            }
        }
        Ui2WindowKind::HostedSurface => {
            if let Some(content) = content_rect {
                if !window.hosted_surface_tiles.is_empty() {
                    let texture_started_at = Instant::now();
                    let drew = draw_hosted_surface_tiles(
                        state,
                        window,
                        content,
                        window_scroll_snapshot(window),
                    );
                    if drew {
                        return Ui2WindowDrawTiming {
                            chrome_ms,
                            texture_ms: elapsed_ms_since(texture_started_at),
                            placeholder_ms: 0,
                            content_path: "surface-tiles",
                        };
                    }
                }
                let texture_drawable = texture_is_drawable(window.content_tex_id);
                if texture_drawable {
                    let texture_started_at = Instant::now();
                    let drew =
                        draw_window_texture_content(state, window, content, window.content_tex_id);
                    if drew {
                        return Ui2WindowDrawTiming {
                            chrome_ms,
                            texture_ms: elapsed_ms_since(texture_started_at),
                            placeholder_ms: 0,
                            content_path: "surface-texture",
                        };
                    }
                }
                let placeholder_started_at = Instant::now();
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Preparing Window",
                    b"Waiting for texture upload",
                );
                return Ui2WindowDrawTiming {
                    chrome_ms,
                    texture_ms: 0,
                    placeholder_ms: elapsed_ms_since(placeholder_started_at),
                    content_path: if texture_drawable {
                        "surface-texture-fallback"
                    } else {
                        "surface-placeholder"
                    },
                };
            }
        }
        Ui2WindowKind::Hosted3d => {
            if let Some(content) = content_rect {
                let texture_drawable = texture_is_drawable(window.content_tex_id);
                if texture_drawable {
                    let texture_started_at = Instant::now();
                    if draw_texture_rect_no_present(
                        window.content_tex_id,
                        content.x,
                        content.y,
                        content.w,
                        content.h,
                        state.view_w,
                        state.view_h,
                        window.content_tex_blend,
                        window.alpha,
                    ) {
                        return Ui2WindowDrawTiming {
                            chrome_ms,
                            texture_ms: elapsed_ms_since(texture_started_at),
                            placeholder_ms: 0,
                            content_path: "3d-texture",
                        };
                    }
                }
                let placeholder_started_at = Instant::now();
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Starting 3D Scene",
                    b"Waiting for scene frame",
                );
                return Ui2WindowDrawTiming {
                    chrome_ms,
                    texture_ms: 0,
                    placeholder_ms: elapsed_ms_since(placeholder_started_at),
                    content_path: if texture_drawable {
                        "3d-texture-fallback"
                    } else {
                        "3d-placeholder"
                    },
                };
            }
        }
    }

    Ui2WindowDrawTiming {
        chrome_ms,
        texture_ms: 0,
        placeholder_ms: 0,
        content_path: "none",
    }
}

fn upload_ui2_blank_target(tex_id: u32, view_w: u32, view_h: u32, reason: &'static str) -> bool {
    let width = view_w.max(1);
    let height = view_h.max(1);

    let Some(pixel_bytes) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        crate::log!("ui2: invalid layer render-target size={}x{}\n", width, height);
        return false;
    };

    let pixels = alloc::vec![0u8; pixel_bytes];
    (unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
            tex_id,
            width,
            height,
            pixels.as_ptr(),
            pixels.len(),
        ) == 0
    }) || {
        crate::log!("ui2: layer render-target upload failed tex={} reason={}\n", tex_id, reason);
        false
    }
}

fn ensure_ui2_layer_render_target(
    tex_id: u32,
    view_w: u32,
    view_h: u32,
    reason: &'static str,
) -> bool {
    let width = view_w.max(1);
    let height = view_h.max(1);

    let mut existing_w = 0u32;
    let mut existing_h = 0u32;
    let already_sized = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_texture_dimensions(
            tex_id,
            &mut existing_w as *mut u32,
            &mut existing_h as *mut u32,
        ) == 0
    } && existing_w == width
        && existing_h == height;
    if already_sized {
        return true;
    }

    upload_ui2_blank_target(tex_id, width, height, reason)
}

fn compose_ui2_frame(state: &mut Ui2State, present_to_screen: bool) -> bool {
    let stats = collect_compose_window_stats(state);
    let compose_seq = state.compose_seq.wrapping_add(1);
    let compose_reason = state.compose_reason;
    let compose_started_ms = boot_probe_ms();
    let compose_started_at = Instant::now();
    let mut surface_timings = Vec::new();
    let mut frame_ok = false;
    let intel_direct_screen = crate::gfx::is_intel_active() && present_to_screen;
    let intel_skip_overlay_layer = crate::gfx::is_intel_active();
    let scene_dirty = !present_to_screen
        || !state.first_compose_signaled
        || state.windows.iter().any(|w| w.dirty);

    if !intel_direct_screen
        && !ensure_ui2_layer_render_target(
            UI2_SCENE_TARGET_TEX_ID,
            state.view_w,
            state.view_h,
            "ui2-scene-target",
        )
    {
        crate::log!("ui2: scene render-target ensure failed\n");
        return false;
    }
    if present_to_screen && !intel_skip_overlay_layer && !intel_direct_screen {
        if !ensure_ui2_layer_render_target(
            UI2_OVERLAY_TARGET_TEX_ID,
            state.view_w,
            state.view_h,
            "ui2-overlay-target",
        ) {
            crate::log!("ui2: overlay render-target ensure failed\n");
            return false;
        }
    }

    let lock_result = crate::gfx::try_with_cabi_frame_lock(20_000, || {
        if intel_direct_screen {
            let content_only_dirty = intel_direct_content_only_dirty(state);
            let primary_clear = if content_only_dirty {
                None
            } else {
                crate::intel::gpgpu::clear_primary_rgba8_white_stats()
            };
            let chrome_gradients_gpgpu = if content_only_dirty {
                true
            } else {
                draw_chrome_gradient_rects_intel_gpgpu(state)
            };
            let chrome_rects_gpgpu = if content_only_dirty {
                true
            } else {
                draw_chrome_solid_rects_intel_gpgpu(state, chrome_gradients_gpgpu)
            };
            let content_gpgpu = draw_window_content_textures_intel_gpgpu(state, content_only_dirty);
            let surroundings_sprite64 = if content_only_dirty {
                if state.chrome_overlay_dirty
                    || state
                        .windows
                        .iter()
                        .any(|window| window_is_renderable(window) && window.chrome_titlebar_dirty)
                {
                    draw_active_chrome_overlay_intel_gpgpu(state)
                } else if state.cursor_overlay_dirty {
                    draw_cursor_overlay_layer_intel_sprite64(
                        state,
                        Ui2ChromeSpriteScope::None,
                        true,
                    )
                } else {
                    true
                }
            } else {
                draw_cursor_overlay_layer_intel_sprite64(state, Ui2ChromeSpriteScope::All, true)
            };
            if compose_seq <= 16 || compose_seq.is_multiple_of(120) {
                crate::log!(
                    "ui2: intel direct-window-present seq={} windows={} hotloop={} scene_layer=0 fullscreen_alpha=0 primary_clear={} clear_descs={} clear_submits={} clear_submit_ms={} clear_present_ms={} clear_total_ms={} content_gpgpu={} chrome_gradients_gpgpu={} chrome_rects_gpgpu={} surroundings_sprite64={}\n",
                    compose_seq,
                    stats.visible_windows,
                    if content_only_dirty {
                        "content-dirty-only"
                    } else {
                        "primary-clear+gradients+rect-lines+content+sprite64"
                    },
                    primary_clear.is_some() as u8,
                    primary_clear.as_ref().map(|stats| stats.spans).unwrap_or(0),
                    primary_clear
                        .as_ref()
                        .map(|stats| stats.submits)
                        .unwrap_or(0),
                    primary_clear
                        .as_ref()
                        .map(|stats| stats.submit_ms)
                        .unwrap_or(0),
                    primary_clear
                        .as_ref()
                        .map(|stats| stats.present_ms)
                        .unwrap_or(0),
                    primary_clear
                        .as_ref()
                        .map(|stats| stats.total_ms)
                        .unwrap_or(0),
                    content_gpgpu as u8,
                    chrome_gradients_gpgpu as u8,
                    chrome_rects_gpgpu as u8,
                    surroundings_sprite64 as u8
                );
            }
            frame_ok = true;
            return;
        }

        if scene_dirty {
            let intel_preserve_scene =
                crate::gfx::is_intel_active() && state.first_compose_signaled;
            let begin_rc = unsafe {
                if intel_preserve_scene {
                    crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve_no_present(0)
                } else {
                    crate::r::io::cabi::trueos_cabi_gfx_begin_frame_no_present(0)
                }
            };
            if begin_rc != 0 {
                crate::log!("ui2: scene begin_frame-no-present failed rc={}\n", begin_rc);
                return;
            }
            let set_rt_rc = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_render_target(UI2_SCENE_TARGET_TEX_ID)
            };
            if set_rt_rc != 0 {
                crate::log!("ui2: scene render-target bind failed rc={}\n", set_rt_rc);
                let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
                return;
            }
            if crate::gfx::is_intel_active() || !intel_preserve_scene {
                let scene_clear_rc = unsafe {
                    crate::r::io::cabi::trueos_cabi_gfx_clear_rgba_no_present(0, 0, 0, 0)
                };
                if scene_clear_rc != 0 {
                    crate::log!(
                        "ui2: scene render-target transparent clear failed rc={}\n",
                        scene_clear_rc
                    );
                    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
                    return;
                }
            }
            ui2_win_register::draw_offline_dock(state);
            for idx in sorted_window_indices(state) {
                let window = &state.windows[idx];
                if !window_is_renderable(window) {
                    continue;
                }
                let timing = draw_window_frame(state, window, false, false, false);
                surface_timings.push(Ui2ComposeSurfaceTiming {
                    id: window.id,
                    chrome_ms: timing.chrome_ms,
                    texture_ms: timing.texture_ms,
                    placeholder_ms: timing.placeholder_ms,
                    path: timing.content_path,
                });
            }
            unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_end_frame();
            }
        }

        if !present_to_screen {
            frame_ok = true;
            return;
        }

        if !intel_skip_overlay_layer {
            let overlay_begin_rc =
                unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame_no_present(0) };
            if overlay_begin_rc != 0 {
                crate::log!("ui2: overlay begin_frame-no-present failed rc={}\n", overlay_begin_rc);
                return;
            }
            let overlay_rt_rc = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_render_target(UI2_OVERLAY_TARGET_TEX_ID)
            };
            if overlay_rt_rc != 0 {
                crate::log!("ui2: overlay render-target bind failed rc={}\n", overlay_rt_rc);
                let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
                return;
            }
            let clear_overlay_rc =
                unsafe { crate::r::io::cabi::trueos_cabi_gfx_clear_rgba_no_present(0, 0, 0, 0) };
            if clear_overlay_rc != 0 {
                crate::log!("ui2: overlay clear failed rc={}\n", clear_overlay_rc);
                let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
                return;
            }
            draw_cursor_overlay_layer(state);
            if scene_dirty {
                draw_resize_preview_outline(state);
            }
            unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_end_frame();
            }
        }

        let present_begin_rc =
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve(0) };
        if present_begin_rc != 0 {
            crate::log!("ui2: layered present begin_frame failed rc={}\n", present_begin_rc);
            return;
        }
        let _ = unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_suppress_cursor_overlay_for_current_frame()
        };
        let scene_drawn = draw_texture_rect_no_present(
            UI2_SCENE_TARGET_TEX_ID,
            0.0,
            0.0,
            state.view_w as f32,
            state.view_h as f32,
            state.view_w,
            state.view_h,
            crate::gfx::is_intel_active(),
            255,
        );
        let overlay_drawn = intel_skip_overlay_layer
            || draw_texture_rect_no_present(
                UI2_OVERLAY_TARGET_TEX_ID,
                0.0,
                0.0,
                state.view_w as f32,
                state.view_h as f32,
                state.view_w,
                state.view_h,
                true,
                255,
            );
        if !scene_drawn || !overlay_drawn {
            crate::log!(
                "ui2: layered present draw failed scene={} overlay={}\n",
                scene_drawn as u8,
                overlay_drawn as u8
            );
            let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            return;
        }
        unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_end_frame();
        }
        frame_ok = true;
    });
    if lock_result.is_none() {
        crate::log!(
            "ui2: compose skipped reason={} present={} dirty_scissor={} cause=cabi-frame-lock-busy\n",
            compose_reason,
            present_to_screen as u8,
            0
        );
    }

    if !frame_ok {
        return false;
    }

    state.compose_seq = compose_seq;
    state.last_logged_compose_seq = compose_seq;
    state.last_logged_compose_reason = compose_reason;
    state.last_logged_compose_dirty_count =
        state.windows.iter().filter(|window| window.dirty).count();
    UI2_DIRTY.store(false, Ordering::Release);
    state.cursor_overlay_dirty = false;
    state.chrome_overlay_dirty = false;
    for window in &mut state.windows {
        window.chrome_titlebar_dirty = false;
        window.chrome_hover_clear_button = None;
        if scene_dirty && window.dirty {
            window.dirty = false;
            window.content_present_dirty = false;
            window.dirty_seq = window.dirty_seq.wrapping_add(1);
            window.last_logged_dirty_seq = window.dirty_seq;
            window.last_logged_reason = window.last_reason;
        }
    }

    if present_to_screen && !state.first_compose_signaled {
        state.first_compose_signaled = true;
        crate::r::readiness::set(crate::r::readiness::UI2_READY);
        crate::log!("boot-probe: ui2 first compose begin ms={}\n", compose_started_ms);
    }

    if crate::logflag::UI2_ENABLE_VERBOSE_COMPOSE_LOGS
        && (compose_seq <= 2 || compose_seq.is_multiple_of(UI2_COMPOSE_LOG_EVERY))
    {
        let present_ms = elapsed_ms_since(compose_started_at);
        crate::log!(
            "ui2: compose-heartbeat seq={} reason={} visible={} browser={} drawable={} pending={} surface={} present_ms={} present={}\n",
            compose_seq,
            compose_reason,
            stats.visible_windows,
            stats.hosted_browser_windows,
            stats.hosted_browser_drawable,
            stats.hosted_browser_pending,
            stats.hosted_surface_windows,
            present_ms,
            if present_to_screen { 1 } else { 0 }
        );
        for timing in &surface_timings {
            crate::log!(
                "ui2: compose-surface-ms seq={} window={} chrome_ms={} texture_ms={} placeholder_ms={} path={}\n",
                compose_seq,
                timing.id,
                timing.chrome_ms,
                timing.texture_ms,
                timing.placeholder_ms,
                timing.path
            );
        }
    }

    true
}

#[embassy_executor::task]
pub async fn ui2_task() {
    if UI2_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::log!("boot-probe: ui2 task start ms={}\n", boot_probe_ms());
    let state_lock = init_state();

    loop {
        let materialized_vm_windows =
            crate::hv::materialize_pending_deferred_blueprint_app_windows();
        if materialized_vm_windows != 0 {
            UI2_DIRTY.store(true, Ordering::Release);
        }

        let mut created_parse_pool_windows = 0usize;
        if let Some(active_mask) = take_hosted_browser_parse_pool_mask() {
            created_parse_pool_windows = sync_hosted_browser_parse_pool_windows(active_mask);
            if created_parse_pool_windows != 0 {
                UI2_DIRTY.store(true, Ordering::Release);
            }
        }
        let hosted_browser_dirty = take_hosted_browser_dirty_mask();

        let mut did_compose = false;
        {
            let mut state = state_lock.lock();
            if materialized_vm_windows != 0 {
                state.compose_reason = "vm-app-window-materialize";
            }
            if created_parse_pool_windows != 0 {
                state.compose_reason = "hosted-browser-parse-pool";
            }

            let athlas_small_ready_seq = crate::gfx::althlasfont::athlas_tier_ready_seq(0);
            if state.last_athlas_small_ready_seq != athlas_small_ready_seq {
                state.last_athlas_small_ready_seq = athlas_small_ready_seq;
                if athlas_small_ready_seq != 0 {
                    state.compose_reason = "athlas-small-ready";
                    UI2_DIRTY.store(true, Ordering::Release);
                }
            }

            pump_cursor_selection(&mut state);
            pump_keyboard_input(&mut state);
            log_browser_surface_updates(&mut state);
            apply_hosted_browser_dirty(&mut state, hosted_browser_dirty);

            let should_compose = !state.first_compose_signaled || UI2_DIRTY.load(Ordering::Acquire);

            if should_compose {
                did_compose = compose_ui2_frame(&mut state, true);
            } else {
                let now_ms = ui2_now_ms();
                if active_chrome_overlay_due(&state, now_ms) {
                    state.last_chrome_overlay_anim_ms = now_ms;
                    did_compose = draw_active_chrome_overlay_intel_gpgpu(&mut state);
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(if did_compose { 16 } else { 10 })).await;
    }
}
