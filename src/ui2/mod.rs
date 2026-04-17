use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::{Mutex, Once};

mod gadget;
mod ui2_browser;
mod ui2_font;
mod ui2_font_bucketproducer;
mod ui2_hid;
mod ui2_hit;
mod ui2_hosted;
mod ui2_win_deco;

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
pub(crate) use self::ui2_hid::{Ui2CursorColor, cursor_color, cursor_color_rgba8};
pub(crate) use self::ui2_hit::ui2_hit_task;
use self::ui2_hit::*;
use self::ui2_hosted::*;
pub(crate) use self::ui2_hosted::{signal_hosted_browser_factory_mask, ui2_hosted_task};
pub use self::ui2_win::*;
pub use self::ui2_win_deco::*;
use trueos_gfx_core::{
    RGB_VERTEX_SIZE, Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_rgb_quad_px, push_tex_quad_px,
};

const UI2_BAR_H: f32 = 26.0;
const UI2_TITLE_H: f32 = UI2_BAR_H;
const UI2_BOTTOM_BAR_H: f32 = UI2_BAR_H;
const UI2_SYSTEM_SCROLLBAR_PX: f32 = 4.0;
const UI2_SYSTEM_BUTTON_FORK_ICON_ID: u32 = 3;
const UI2_SYSTEM_BUTTON_MINIMIZE_ICON_ID: u32 = 5;
const UI2_SYSTEM_BUTTON_MAXIMIZE_ICON_ID: u32 = 7;
const UI2_SYSTEM_BUTTON_CLOSE_ICON_ID: u32 = 9;
// Experimental in debug.
const UI2_BROWSER_TITLE_OVERLAY_ANIMATION_ENABLED: bool = false;
const UI2_DEBUG_FPS_OVERLAY_ENABLED: bool = false;
const UI2_BROWSER_TITLE_OVERLAY_PERIOD_MS: u64 = 2400;
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
const UI2_WINDOW_UPDATE_LOG_EVERY: u32 = 32;
const UI2_COMPOSE_LOG_EVERY: u32 = 32;
const UI2_WINDOW_RESIZE_LEFT: u32 = 1 << 0;
const UI2_WINDOW_RESIZE_TOP: u32 = 1 << 1;
const UI2_WINDOW_RESIZE_RIGHT: u32 = 1 << 2;
const UI2_WINDOW_RESIZE_BOTTOM: u32 = 1 << 3;
const UI2_WARMUP_RENDER_TARGET_TEX_ID: u32 = 4_706;

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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2SystemButtonAction {
    ToggleComposition,
    Fork,
    Minimize,
    ToggleMaximize,
    Close,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2WindowKind {
    HostedBrowser,
    HostedSurface,
    Hosted3d,
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

#[derive(Clone)]
struct Ui2Window {
    id: u32,
    kind: Ui2WindowKind,
    browser_instance_id: u32,
    hosted_browser_snapshot: UiHostedBrowserSnapshot,
    title: String,
    icon_id: u32,
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
    left_scrollbar_visible: bool,
    bottom_scrollbar_visible: bool,
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
    title_tex_id: u32,
    title_tex_w: u32,
    title_tex_h: u32,
    title_tex_alpha: u8,
    container_sync_needed: bool,
    selected_cursor_slots: Vec<u32>,
    dirty: bool,
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
    if window.state == Ui2WindowStateKind::Minimized {
        2
    } else {
        5
    }
}

#[inline]
fn ui2_window_min_size(window: &Ui2Window) -> (f32, f32) {
    if window.decoration_mode != Ui2WindowDecorationMode::System {
        return (1.0, 1.0);
    }

    let mut min_w: f32 = 1.0;
    let min_h = if window.titlebar_visible {
        UI2_TITLE_H
    } else {
        0.0
    } + if window.bottom_bar_visible {
        UI2_BOTTOM_BAR_H
    } else {
        0.0
    };

    let button_count = ui2_system_button_count(window);
    if button_count != 0 {
        let s = UI2_TITLE_H;
        let gap = 1.0f32;
        let button_span = button_count as f32 * s + button_count.saturating_sub(1) as f32 * gap;
        min_w = min_w.max(button_span);
    }

    if window.bottom_bar_visible && window.state == Ui2WindowStateKind::Normal {
        min_w = min_w.max(UI2_BOTTOM_BAR_H + 1.0);
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
    compose_present_history_ms: Vec<u64>,
    compose_fps_display: u16,
    last_compose_heartbeat_seq: u32,
    last_athlas_small_ready_seq: u32,
    first_compose_signaled: bool,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui2ComposeWindowStats {
    visible_windows: usize,
    hosted_browser_windows: usize,
    hosted_browser_drawable: usize,
    hosted_browser_pending: usize,
    hosted_surface_windows: usize,
}

const UI2_WINDOW_TITLE_TEX_ID_BASE: u32 = 20_000;
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
fn browser_title_overlay_mid_offset(window: &Ui2Window, now_ms: u64) -> f32 {
    if window.kind != Ui2WindowKind::HostedBrowser || !UI2_BROWSER_TITLE_OVERLAY_ANIMATION_ENABLED {
        return 0.5;
    }

    let period_ms = UI2_BROWSER_TITLE_OVERLAY_PERIOD_MS.max(1);
    let phase_ms = now_ms
        .wrapping_add((window_browser_instance_id(window) as u64).saturating_mul(period_ms / 5))
        % period_ms;
    let t = phase_ms as f32 / period_ms as f32;
    0.18 + (0.64 * t)
}

fn init_state() -> &'static Mutex<Ui2State> {
    UI2_STATE.call_once(|| {
        let (view_w, view_h) = crate::intel::active_scanout_dimensions()
            .or_else(|| {
                crate::limine::framebuffer_response()
                    .and_then(|resp| resp.framebuffers().next())
                    .map(|fb| (fb.width() as u32, fb.height() as u32))
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
            compose_present_history_ms: Vec::new(),
            compose_fps_display: 0,
            last_compose_heartbeat_seq: 0,
            last_athlas_small_ready_seq: 0,
            first_compose_signaled: false,
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

fn window_kind_id(kind: Ui2WindowKind) -> u32 {
    match kind {
        Ui2WindowKind::HostedBrowser => 1,
        Ui2WindowKind::HostedSurface => 3,
        Ui2WindowKind::Hosted3d => 4,
    }
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

fn note_window_dirty(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.dirty = true;
    window.last_reason = reason;
    UI2_DIRTY.store(true, Ordering::Release);
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
    signal_hosted_browser_dirty(content_id, flags);
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
    create_window(
        title,
        rect,
        z.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        alpha.min(255) as u8,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_info(
    window_id: u32,
    out_info: *mut TrueosUi2WindowInfo,
) -> i32 {
    if out_info.is_null() {
        return -1;
    }
    let Some(info) = window_info_by_id(window_id) else {
        return -1;
    };
    *out_info = info;
    0
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
    if set_window_title(window_id, title) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_set_icon(window_id: u32, icon_id: u32) -> i32 {
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
    if resize_window(window_id, width as f32, height as f32) {
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
    if set_window_decorations(window_id, mode) {
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
    if set_window_hit_test_visible(window_id, visible != 0) {
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
    if set_window_horizontal_scrollbar_side(window_id, side) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_minimize(window_id: u32) -> i32 {
    if minimize_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_maximize(window_id: u32) -> i32 {
    if maximize_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_restore(window_id: u32) -> i32 {
    if restore_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_focus(window_id: u32) -> i32 {
    if focus_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_close(window_id: u32) -> i32 {
    if close_window(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_begin_move(window_id: u32) -> i32 {
    if begin_window_move(window_id) { 0 } else { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_window_begin_resize(
    window_id: u32,
    edge_mask: u32,
) -> i32 {
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
fn window_title_tex_id(window_id: u32) -> u32 {
    UI2_WINDOW_TITLE_TEX_ID_BASE.saturating_add(window_id.saturating_sub(1))
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

fn draw_window_frame(state: &Ui2State, window: &Ui2Window) -> Ui2WindowDrawTiming {
    if !window_is_renderable(window) {
        return Ui2WindowDrawTiming::default();
    }

    let chrome_started_at = Instant::now();
    let rect = effective_window_rect(state, window);
    let content_rect = window_content_rect(state, window);
    draw_window_chrome(state, window, rect);

    let chrome_ms = elapsed_ms_since(chrome_started_at);

    if !window_content_participates_in_composition(window) {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: 0,
            placeholder_ms: 0,
            content_path: "locked",
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
                    let drew = draw_texture_rect_no_present(
                        window.content_tex_id,
                        content.x,
                        content.y,
                        content.w,
                        content.h,
                        state.view_w,
                        state.view_h,
                        window.content_tex_blend,
                        window.alpha,
                    );
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

fn ensure_ui2_warmup_render_target(view_w: u32, view_h: u32) -> bool {
    let width = view_w.max(1);
    let height = view_h.max(1);

    let mut existing_w = 0u32;
    let mut existing_h = 0u32;
    let already_sized = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_texture_dimensions(
            UI2_WARMUP_RENDER_TARGET_TEX_ID,
            &mut existing_w as *mut u32,
            &mut existing_h as *mut u32,
        ) == 0
    } && existing_w == width
        && existing_h == height;
    if already_sized {
        return true;
    }

    let Some(pixel_bytes) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        crate::log!("ui2: invalid warmup render-target size={}x{}\n", width, height);
        return false;
    };

    let pixels = alloc::vec![0u8; pixel_bytes];
    crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        UI2_WARMUP_RENDER_TARGET_TEX_ID,
        width,
        height,
        pixels.as_slice(),
        0,
        "ui2-warmup-render-target",
    )
}

fn compose_ui2_frame(state: &mut Ui2State, present_to_screen: bool) -> bool {
    let stats = collect_compose_window_stats(state);
    let compose_seq = state.compose_seq.wrapping_add(1);
    let compose_reason = state.compose_reason;
    let compose_started_ms = boot_probe_ms();
    let compose_started_at = Instant::now();
    let mut surface_timings = Vec::new();
    let mut frame_ok = false;

    if !present_to_screen && !ensure_ui2_warmup_render_target(state.view_w, state.view_h) {
        crate::log!("ui2: warmup render-target ensure failed\n");
        return false;
    }

    crate::gfx::with_cabi_frame_lock(|| {
        let begin_rc = unsafe {
            if present_to_screen {
                crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xE9EEF2)
            } else {
                crate::r::io::cabi::trueos_cabi_gfx_begin_frame_no_present(0xE9EEF2)
            }
        };
        if begin_rc != 0 {
            crate::log!(
                "ui2: begin_frame{} failed rc={}\n",
                if present_to_screen { "" } else { "-no-present" },
                begin_rc
            );
            return;
        }

        if !present_to_screen {
            let set_rt_rc = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_render_target(
                    UI2_WARMUP_RENDER_TARGET_TEX_ID,
                )
            };
            if set_rt_rc != 0 {
                crate::log!("ui2: warmup render-target bind failed rc={}\n", set_rt_rc);
                let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
                return;
            }
        }

        for idx in sorted_window_indices(state) {
            let window = &state.windows[idx];
            if !window_is_renderable(window) {
                continue;
            }
            let timing = draw_window_frame(state, window);
            surface_timings.push(Ui2ComposeSurfaceTiming {
                id: window.id,
                chrome_ms: timing.chrome_ms,
                texture_ms: timing.texture_ms,
                placeholder_ms: timing.placeholder_ms,
                path: timing.content_path,
            });
        }
        draw_resize_preview_outline(state);
        unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_end_frame();
        }
        frame_ok = true;
    });

    if !frame_ok {
        return false;
    }

    state.compose_seq = compose_seq;
    state.last_logged_compose_seq = compose_seq;
    state.last_logged_compose_reason = compose_reason;
    state.last_logged_compose_dirty_count =
        state.windows.iter().filter(|window| window.dirty).count();
    UI2_DIRTY.store(false, Ordering::Release);
    for window in &mut state.windows {
        if window.dirty {
            window.dirty = false;
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
        state.compose_present_history_ms.push(present_ms);
        if state.compose_present_history_ms.len() > 64 {
            let excess = state.compose_present_history_ms.len() - 64;
            state.compose_present_history_ms.drain(..excess);
        }
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
        let mut created_factory_windows = 0usize;
        if let Some(active_mask) = take_hosted_browser_factory_mask() {
            created_factory_windows = sync_hosted_browser_factory_windows(active_mask);
            if created_factory_windows != 0 {
                UI2_DIRTY.store(true, Ordering::Release);
            }
        }
        let hosted_browser_dirty = take_hosted_browser_dirty_mask();

        let mut did_compose = false;
        {
            let mut state = state_lock.lock();
            if created_factory_windows != 0 {
                state.compose_reason = "hosted-browser-factory";
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
            }
        }

        Timer::after(EmbassyDuration::from_millis(if did_compose { 16 } else { 10 })).await;
    }
}
