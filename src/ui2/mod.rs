#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

mod ui2_hid;
mod ui2_hit;
mod ui2_hosted;
mod ui2_win_deco;

mod ui2_win;

use self::ui2_hid::*;
pub(crate) use self::ui2_hit::ui2_hit_task;
use self::ui2_hit::*;
use self::ui2_hosted::*;
pub(crate) use self::ui2_hosted::{signal_hosted_browser_factory_mask, ui2_hosted_task};
pub use self::ui2_win::*;
pub use self::ui2_win_deco::*;

const UI2_TITLE_H: f32 = 26.0;
const UI2_BOTTOM_BAR_H: f32 = 18.0;
const UI2_SYSTEM_SCROLLBAR_PX: f32 = 4.0;
const UI2_SYSTEM_BUTTON_W: f32 = 24.0;
const UI2_SYSTEM_BUTTON_H: f32 = 14.0;
const UI2_SYSTEM_BUTTON_GAP: f32 = 4.0;
const UI2_SYSTEM_BUTTON_FORK_ICON_ID: u32 = 3;
const UI2_SYSTEM_BUTTON_MINIMIZE_ICON_ID: u32 = 5;
const UI2_SYSTEM_BUTTON_MAXIMIZE_ICON_ID: u32 = 7;
const UI2_SYSTEM_BUTTON_CLOSE_ICON_ID: u32 = 9;
// Experimental in debug.
const UI2_BROWSER_TITLE_OVERLAY_ANIMATION_ENABLED: bool = false;
const UI2_DEBUG_FPS_OVERLAY_ENABLED: bool = false;
const UI2_BROWSER_TITLE_OVERLAY_PERIOD_MS: u64 = 2400;
const UI2_BROWSER_FORK_WINDOW_OFFSET_PX: f32 = 24.0;
const UI2_BOTTOM_RESIZE_BUTTON_W: f32 = 18.0;
const UI2_BOTTOM_RESIZE_BUTTON_H: f32 = 14.0;
const UI2_BOTTOM_RESIZE_BUTTON_PAD: f32 = 2.0;
const UI2_MINIMIZED_STRIP_W: f32 = 168.0;
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

#[inline]
const fn ui2_window_min_extent() -> f32 {
    UI2_BOTTOM_BAR_H
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Ui2TexVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

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
    press_armed: bool,
    selected_window_id: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui2SystemButtonAction {
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

#[derive(Clone)]
struct Ui2Window {
    id: u32,
    kind: Ui2WindowKind,
    browser_instance_id: u32,
    hosted_browser_snapshot: UiHostedBrowserSnapshot,
    title: String,
    icon_id: u32,
    rect: Ui2Rect,
    restore_rect: Ui2Rect,
    z: i16,
    visible: bool,
    hit_test_visible: bool,
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
    container_sync_needed: bool,
    selected_cursor_slots: Vec<u32>,
    dirty: bool,
    dirty_seq: u32,
    last_reason: &'static str,
    last_logged_dirty_seq: u32,
    last_logged_reason: &'static str,
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
    loadscreen_release_requested: bool,
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
        let (view_w, view_h) = crate::limine::framebuffer_response()
            .and_then(|resp| resp.framebuffers().next())
            .map(|fb| (fb.width() as u32, fb.height() as u32))
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
            loadscreen_release_requested: false,
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

fn browser_surface_state_for_window(window: &Ui2Window) -> UiHostedSurfaceState {
    window.hosted_browser_snapshot.surface
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
        .fold(0u32, |acc, value| {
            acc.wrapping_mul(16777619).wrapping_add(value)
        })
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
        if content_dirty {
            window.dirty = true;
            window.last_reason = "browser-content";
            UI2_DIRTY.store(true, Ordering::Release);
            state.compose_reason = "browser-content";
        }
        if interactive_dirty {
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
        (
            (right_edge - cursor_x).abs(),
            Ui2WindowEdgeDropAction::SnapRight,
        ),
        (cursor_y.abs(), Ui2WindowEdgeDropAction::Maximize),
        (
            (bottom_edge - cursor_y).abs(),
            Ui2WindowEdgeDropAction::Minimize,
        ),
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
        Ui2WindowEdgeDropAction::SnapRight => set_window_rect_in_state(
            state,
            id,
            right_half_window_rect(state),
            "window-snap-right",
        ),
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
pub unsafe extern "C" fn trueos_cabi_ui2_primary_browser_window_id() -> u32 {
    browser_window_id().unwrap_or(0)
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
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let left = (2.0 * (x / vw)) - 1.0;
    let right = (2.0 * ((x + width) / vw)) - 1.0;
    let top = 1.0 - (2.0 * (y / vh));
    let bottom = 1.0 - (2.0 * ((y + height) / vh));
    let verts = [
        Ui2TexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        Ui2TexVertex {
            x: right,
            y: bottom,
            u: u1,
            v: v1,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        Ui2TexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
        Ui2TexVertex {
            x: left,
            y: top,
            u: u0,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: alpha,
        },
    ];
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
            verts.as_ptr() as *const u8,
            verts.len() * core::mem::size_of::<Ui2TexVertex>(),
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

#[inline]
fn snap_browser_content_rect(content: Ui2Rect) -> (i32, i32, u32, u32) {
    (
        libm::roundf(content.x) as i32,
        libm::roundf(content.y) as i32,
        round_to_u32(content.w, 1),
        round_to_u32(content.h, 1),
    )
}

fn queue_browser_window_viewport(content_id: HostedContentId, content: Ui2Rect) -> bool {
    let (content_x, content_y, viewport_w, viewport_h) = snap_browser_content_rect(content);
    hosted_set_viewport(
        content_id, viewport_w, viewport_h, content_x, content_y, viewport_w, viewport_h,
    )
}

#[inline]
fn texture_is_drawable(tex_id: u32) -> bool {
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    tex_id != 0
        && crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id) == ASYNC_TEX_STATUS_READY
}

fn draw_window_content_placeholder(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
    headline: &[u8],
    subline: &[u8],
) {
    let panel_rgba = modulate_rgba_alpha((0xF1, 0xF4, 0xF7, 0xFF), window.alpha);
    let stripe_rgba = modulate_rgba_alpha((0xC6, 0xD1, 0xDB, 0xFF), window.alpha);
    let headline_alpha = modulate_alpha(0xD8, window.alpha);
    let subline_alpha = modulate_alpha(0x9A, window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );
    let stripe_w = libm::fminf(content.w, 96.0);
    let stripe_h = libm::fminf(content.h, 3.0);
    let stripe_x = content.x + ((content.w - stripe_w) * 0.5);
    let stripe_y = content.y + 28.0;
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        stripe_x,
        stripe_y,
        stripe_w,
        stripe_h,
        stripe_rgba,
        state.view_w,
        state.view_h,
    );

    let headline_w = crate::gfx::imba_athlas::imba_athlas_text_width_px(headline);
    let headline_x = content.x + ((content.w - headline_w) * 0.5).max(10.0);
    let headline_y = content.y + 40.0;
    crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
        headline,
        headline_x,
        headline_y,
        state.view_w,
        state.view_h,
        headline_alpha,
    );

    let subline_w = crate::gfx::imba_athlas::imba_athlas_text_width_px(subline);
    let subline_x = content.x + ((content.w - subline_w) * 0.5).max(10.0);
    let subline_y = headline_y + 20.0;
    crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
        subline,
        subline_x,
        subline_y,
        state.view_w,
        state.view_h,
        subline_alpha,
    );
}

fn log_browser_surface_updates(state: &mut Ui2State) {
    let windows = state.windows.clone();
    for window in &windows {
        if window.kind != Ui2WindowKind::HostedBrowser || !window.visible {
            continue;
        }
        let snapshot = browser_surface_state_for_window(window);
        if let Some(content) = window_content_rect(state, window) {
            let (_, _, want_w, want_h) = snap_browser_content_rect(content);
            if snapshot.viewport_width != want_w || snapshot.viewport_height != want_h {
                let _ = note_window_viewport_sync_needed(state, window.id);
                crate::log!(
                    "ui2: browser-viewport-mismatch window={} have={}x{} want={}x{}\n",
                    window.id,
                    snapshot.viewport_width,
                    snapshot.viewport_height,
                    want_w,
                    want_h
                );
            }
        }
    }
}

fn truncate_hosted_browser_text_preview_row(text: &str, max_width_px: f32, px_h: f32) -> Vec<u8> {
    if text.is_empty() || max_width_px <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(text.len().min(96));
    for &byte in text.as_bytes() {
        let normalized = match byte {
            b'\n' | b'\r' | b'\t' => b' ',
            _ => byte,
        };
        out.push(normalized);
        let width = crate::gfx::imba_athlas::imba_athlas_text_width_scaled_px(&out, px_h);
        if width > max_width_px {
            out.pop();
            break;
        }
    }

    while out.last() == Some(&b' ') {
        out.pop();
    }

    out
}

fn draw_hosted_browser_text_preview(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
) -> bool {
    let text_state = &window.hosted_browser_snapshot.text;
    if text_state.rows.is_empty() {
        return false;
    }

    let surface_state = browser_surface_state_for_window(window);
    let panel_rgba = modulate_rgba_alpha((0xFB, 0xFB, 0xF8, 0xFF), window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    let pad_x = 10.0f32;
    let pad_y = 8.0f32;
    let text_px_h = 7.0f32;
    let row_step = text_px_h + 4.0;
    let visible_bottom = content.y + content.h - pad_y;
    let scroll_x = surface_state.scroll_x as f32;
    let scroll_y = surface_state.scroll_y as f32;
    let content_right = content.x + content.w - pad_x;
    let mut drew_any = false;

    for (row_index, row) in text_state.rows.iter().enumerate() {
        let x = content.x + pad_x + row.indent_px as f32 - scroll_x;
        let y = content.y + pad_y + (row_index as f32 * row_step) - scroll_y;
        if y + text_px_h <= content.y || y >= visible_bottom {
            continue;
        }
        if x >= content_right {
            continue;
        }

        let max_width_px = (content_right - x).max(0.0);
        let row_bytes =
            truncate_hosted_browser_text_preview_row(&row.text, max_width_px, text_px_h);
        if row_bytes.is_empty() {
            continue;
        }

        drew_any |= crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha_scaled(
            &row_bytes,
            x,
            y,
            state.view_w,
            state.view_h,
            text_px_h,
            window.alpha,
        );
    }

    drew_any
}

fn draw_hosted_browser_layout_preview(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
) -> bool {
    let layout_state = &window.hosted_browser_snapshot.layout;
    if layout_state.nodes.is_empty() {
        return false;
    }

    let panel_rgba = modulate_rgba_alpha((0xF8, 0xF8, 0xF4, 0xFF), window.alpha);
    let text_rgb = (0x14, 0x18, 0x1D);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    let mut y_cursor = content.y + 8.0;
    let bottom = content.y + content.h - 4.0;
    let mut drew_any = false;

    for node in layout_state.nodes.iter().take(24) {
        if node.parent_id == 0 {
            continue;
        }
        let margin_top = node.margin_top_px as f32;
        let margin_bottom = node.margin_bottom_px as f32;
        let text_top = y_cursor + margin_top;
        if text_top >= bottom {
            continue;
        }
        let text = if !node.tag.is_empty() {
            format!("<{}> </{}>", node.tag, node.tag)
        } else if !node.text.is_empty() {
            node.text.clone()
        } else {
            String::new()
        };
        let line_h = node
            .intrinsic_height_px
            .max(node.min_height_px)
            .clamp(12, 24) as f32;
        let block_h = node.intrinsic_height_px.max(node.min_height_px).max(10) as f32
            + margin_top
            + margin_bottom
            + node.padding_top_px as f32
            + node.padding_bottom_px as f32;
        if !text.is_empty() {
            let left = content.x + 8.0 + node.margin_left_px as f32 + node.padding_left_px as f32;
            let max_width_px = (content.x + content.w - 8.0 - left).max(0.0);
            let row_bytes = truncate_hosted_browser_text_preview_row(&text, max_width_px, line_h);
            if !row_bytes.is_empty() {
                let _ = text_rgb;
                drew_any |= crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha_scaled(
                    &row_bytes,
                    left,
                    text_top,
                    state.view_w,
                    state.view_h,
                    line_h,
                    window.alpha,
                );
            }
        }
        y_cursor += block_h.max(8.0);
    }

    drew_any
}

fn draw_window_frame(state: &Ui2State, window: &Ui2Window) -> Ui2WindowDrawTiming {
    if !window_is_renderable(window) {
        return Ui2WindowDrawTiming::default();
    }

    let frame_started_ms = boot_probe_ms();
    let rect = effective_window_rect(state, window);
    let content_rect = window_content_rect(state, window);
    draw_window_chrome(state, window, rect);

    let chrome_ms = boot_probe_ms().saturating_sub(frame_started_ms);

    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            if let Some(content) = content_rect {
                let content_started_ms = boot_probe_ms();
                if draw_hosted_browser_layout_preview(state, window, content) {
                    return Ui2WindowDrawTiming {
                        chrome_ms,
                        texture_ms: boot_probe_ms().saturating_sub(content_started_ms),
                        placeholder_ms: 0,
                        content_path: "browser-preview",
                    };
                }
                if draw_hosted_browser_text_preview(state, window, content) {
                    return Ui2WindowDrawTiming {
                        chrome_ms,
                        texture_ms: boot_probe_ms().saturating_sub(content_started_ms),
                        placeholder_ms: 0,
                        content_path: "browser-preview-text",
                    };
                }
                if texture_is_drawable(window.content_tex_id)
                    && draw_texture_rect_no_present(
                        window.content_tex_id,
                        content.x,
                        content.y,
                        content.w,
                        content.h,
                        state.view_w,
                        state.view_h,
                        true,
                        window.alpha,
                    )
                {
                    return Ui2WindowDrawTiming {
                        chrome_ms,
                        texture_ms: boot_probe_ms().saturating_sub(content_started_ms),
                        placeholder_ms: 0,
                        content_path: "browser-texture",
                    };
                }
                let placeholder_started_ms = boot_probe_ms();
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Starting Browser Tab",
                    b"Waiting for texture frame",
                );
                return Ui2WindowDrawTiming {
                    chrome_ms,
                    texture_ms: 0,
                    placeholder_ms: boot_probe_ms().saturating_sub(placeholder_started_ms),
                    content_path: "browser-placeholder",
                };
            }
        }
        Ui2WindowKind::HostedSurface => {
            if let Some(content) = content_rect {
                let texture_drawable = texture_is_drawable(window.content_tex_id);
                if texture_drawable {
                    let texture_started_ms = boot_probe_ms();
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
                            texture_ms: boot_probe_ms().saturating_sub(texture_started_ms),
                            placeholder_ms: 0,
                            content_path: "surface-texture",
                        };
                    }
                }
                let placeholder_started_ms = boot_probe_ms();
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
                    placeholder_ms: boot_probe_ms().saturating_sub(placeholder_started_ms),
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
                    let texture_started_ms = boot_probe_ms();
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
                            texture_ms: boot_probe_ms().saturating_sub(texture_started_ms),
                            placeholder_ms: 0,
                            content_path: "3d-texture",
                        };
                    }
                }
                let placeholder_started_ms = boot_probe_ms();
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
                    placeholder_ms: boot_probe_ms().saturating_sub(placeholder_started_ms),
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

    let pixels =
        alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
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
        crate::log!(
            "boot-probe: ui2 first compose begin ms={}\n",
            compose_started_ms
        );
    }

    if compose_seq <= 2 || compose_seq.is_multiple_of(UI2_COMPOSE_LOG_EVERY) {
        let present_ms = boot_probe_ms().saturating_sub(compose_started_ms);
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

            pump_cursor_selection(&mut state);
            pump_keyboard_input(&mut state);
            log_browser_surface_updates(&mut state);
            apply_hosted_browser_dirty(&mut state, hosted_browser_dirty);

            let loadscreen_ended = crate::r::readiness::is_set(crate::r::readiness::LOADSCREEN_END);
            let should_compose = if loadscreen_ended {
                !state.first_compose_signaled || UI2_DIRTY.load(Ordering::Acquire)
            } else {
                !state.loadscreen_release_requested || UI2_DIRTY.load(Ordering::Acquire)
            };

            if should_compose {
                did_compose = compose_ui2_frame(&mut state, loadscreen_ended);
                if did_compose && !loadscreen_ended && !state.loadscreen_release_requested {
                    state.loadscreen_release_requested = true;
                    crate::r::readiness::set_loadscreen_expire_requested(true);
                    crate::log!(
                        "boot-probe: ui2 requested loadscreen release ms={}\n",
                        boot_probe_ms()
                    );
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(if did_compose {
            16
        } else {
            10
        }))
        .await;
    }
}
