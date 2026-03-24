#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use parry2d::math::{Isometry, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};
use spin::{Mutex, Once};

mod ui2_hid;
mod ui2_hosted;
mod ui2_win_deco;

mod ui2_win;

pub(crate) use self::ui2_hosted::signal_hosted_browser_factory_mask;
use self::ui2_hosted::*;
pub use self::ui2_win::*;
pub use self::ui2_win_deco::*;

const UI2_TITLE_H: f32 = 26.0;
const UI2_BOTTOM_BAR_H: f32 = 18.0;
const UI2_SYSTEM_SCROLLBAR_PX: f32 = 4.0;
const UI2_SYSTEM_BUTTON_W: f32 = 24.0;
const UI2_SYSTEM_BUTTON_H: f32 = 14.0;
const UI2_SYSTEM_BUTTON_GAP: f32 = 4.0;
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Ui2HitKind {
    WindowBody,
    WindowDecoration,
    WindowResizeButton,
    WindowVerticalScrollbar,
    WindowHorizontalScrollbar,
    BrowserInteractive,
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
}

#[derive(Copy, Clone, Debug)]
struct Ui2HitEntry {
    owner_window_id: u32,
    item_id: u32,
    kind: Ui2HitKind,
    rect: Ui2Rect,
    z: i16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Ui2HitTarget {
    owner_window_id: u32,
    item_id: u32,
    kind: Ui2HitKind,
}

#[derive(Default)]
struct Ui2HitScene {
    seq: u32,
    entries: Vec<Ui2HitEntry>,
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
    hit_scene: Ui2HitScene,
    last_browser_interactive_seq: u32,
    move_drags: Vec<Ui2WindowMoveDrag>,
    resize_drags: Vec<Ui2WindowResizeDrag>,
    scroll_drags: Vec<Ui2WindowScrollDrag>,
    scroll_pan_drags: Vec<Ui2WindowScrollPanDrag>,
    windows: Vec<Ui2Window>,
    compose_present_history_ms: Vec<u64>,
    compose_fps_display: u16,
    first_compose_signaled: bool,
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

fn hosted_browser_factory_content_rect_for_view(
    view_w: u32,
    view_h: u32,
    slot: u32,
    total: u32,
) -> Ui2Rect {
    let cols = if total >= 2 { 2u32 } else { 1u32 };
    let rows = total.div_ceil(cols).max(1);
    let margin_x = 48.0f32;
    let margin_y = 84.0f32;
    let gutter = 18.0f32;
    let bottom_margin = 36.0f32;
    let usable_w = (view_w as f32) - margin_x * 2.0 - gutter * (cols.saturating_sub(1) as f32);
    let usable_h =
        (view_h as f32) - margin_y - bottom_margin - gutter * (rows.saturating_sub(1) as f32);
    let width = (usable_w / cols as f32).clamp(520.0, 960.0);
    let height = (usable_h / rows as f32).clamp(320.0, 640.0);
    let col = slot % cols;
    let row = slot / cols;
    Ui2Rect::new(
        margin_x + col as f32 * (width + gutter),
        margin_y + row as f32 * (height + gutter),
        width,
        height,
    )
}

fn sync_hosted_browser_factory_windows(active_mask: u32) -> usize {
    if active_mask == 0 {
        return 0;
    }

    let active_ids: Vec<u32> = trueos_qjs::browser_task::BOOT_BROWSER_INSTANCE_IDS
        .iter()
        .copied()
        .filter(|browser_instance_id| {
            let bit = 1u32 << browser_instance_id.saturating_sub(1);
            (active_mask & bit) != 0
        })
        .collect();
    if active_ids.is_empty() {
        return 0;
    }

    let (view_w, view_h) = {
        let state_lock = init_state();
        let state = state_lock.lock();
        (state.view_w, state.view_h)
    };

    let total = active_ids.len() as u32;
    let mut created = 0usize;
    for (slot, browser_instance_id) in active_ids.into_iter().enumerate() {
        if hosted_window_id_for_content(browser_instance_id) != 0 {
            continue;
        }
        let title = format!("Truesurfer {}", browser_instance_id);
        let content_rect =
            hosted_browser_factory_content_rect_for_view(view_w, view_h, slot as u32, total);
        let tex_id =
            trueos_qjs::browser_task::render_tex_id_for_browser_instance(browser_instance_id);
        let window_id = create_hosted_browser_content_window(
            title.as_str(),
            content_rect,
            40i16.saturating_add(slot as i16),
            255,
            browser_instance_id,
            tex_id,
        );
        crate::log!(
            "ui2: hosted-browser-factory window={} browser={} tex={} slot={} total={}\n",
            window_id,
            browser_instance_id,
            tex_id,
            slot,
            total
        );
        created = created.saturating_add(1);
    }
    created
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
            cursors: Vec::new(),
            hit_scene: Ui2HitScene::default(),
            last_browser_interactive_seq: 0,
            move_drags: Vec::new(),
            resize_drags: Vec::new(),
            scroll_drags: Vec::new(),
            scroll_pan_drags: Vec::new(),
            windows: Vec::new(),
            compose_present_history_ms: Vec::new(),
            compose_fps_display: 0,
            first_compose_signaled: false,
        };

        refresh_all_window_hit_entries(&mut state);

        Mutex::new(state)
    })
}

fn alloc_window(
    state: &mut Ui2State,
    kind: Ui2WindowKind,
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
) -> u32 {
    let id = state.next_window_id;
    state.next_window_id = state.next_window_id.wrapping_add(1).max(1);
    state.windows.push(Ui2Window {
        id,
        kind,
        browser_instance_id: if kind == Ui2WindowKind::HostedBrowser {
            PRIMARY_HOSTED_CONTENT_ID
        } else {
            0
        },
        title: String::from(title),
        icon_id: 0,
        rect,
        restore_rect: rect,
        z,
        visible: true,
        hit_test_visible: true,
        alpha,
        decoration_mode: Ui2WindowDecorationMode::System,
        titlebar_visible: true,
        bottom_bar_visible: true,
        left_scrollbar_visible: true,
        bottom_scrollbar_visible: true,
        vertical_scrollbar_side: Ui2WindowVerticalScrollbarSide::Left,
        horizontal_scrollbar_side: Ui2WindowHorizontalScrollbarSide::Bottom,
        state: Ui2WindowStateKind::Normal,
        content_tex_id: 0,
        content_tex_blend: false,
        container_sync_needed: true,
        selected_cursor_slots: Vec::new(),
        dirty: true,
        dirty_seq: 0,
        last_reason: "create",
        last_logged_dirty_seq: 0,
        last_logged_reason: "",
    });
    id
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
    let mut snapshot = hosted_surface_state(window_browser_instance_id(window));
    if snapshot.content_width == 0 {
        snapshot.content_width = snapshot.viewport_width.max(1);
    }
    if snapshot.content_height == 0 {
        snapshot.content_height = snapshot.viewport_height.max(1);
    }
    snapshot
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
    hosted_interactive_state(window_browser_instance_id(window))
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

fn hosted_browser_content_seq(state: &Ui2State) -> u32 {
    state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| {
            let instance_id = window_browser_instance_id(window);
            let surface_seq = hosted_surface_seq(instance_id);
            let text_seq = hosted_text_seq(instance_id);
            instance_id
                .wrapping_mul(1315423911)
                .wrapping_add(surface_seq)
                .wrapping_mul(16777619)
                .wrapping_add(text_seq)
        })
        .fold(0u32, |acc, value| {
            acc.wrapping_mul(16777619).wrapping_add(value)
        })
}

fn window_is_renderable(window: &Ui2Window) -> bool {
    window.visible
}

struct Ui2HitBuildContext<'a> {
    state: &'a Ui2State,
}

trait Ui2WindowHitSource {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene);
}

impl Ui2WindowHitSource for Ui2Window {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene) {
        if !window_is_renderable(self) || !self.hit_test_visible {
            return;
        }

        let rect = effective_window_rect(ctx.state, self);

        scene.append(Ui2HitEntry {
            owner_window_id: self.id,
            item_id: 0,
            kind: Ui2HitKind::WindowBody,
            rect,
            z: self.z,
        });
        if let Some(rect) = window_decoration_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 1,
                kind: Ui2HitKind::WindowDecoration,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_bottom_resize_button_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 2,
                kind: Ui2HitKind::WindowResizeButton,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_vertical_scrollbar_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 3,
                kind: Ui2HitKind::WindowVerticalScrollbar,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_horizontal_scrollbar_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 4,
                kind: Ui2HitKind::WindowHorizontalScrollbar,
                rect,
                z: self.z,
            });
        }

        match self.kind {
            Ui2WindowKind::HostedBrowser => {
                let Some(content) = window_content_rect(ctx.state, self) else {
                    return;
                };
                let browser_interactives = browser_interactive_state_for_window(self);
                for interactive in &browser_interactives.interactives {
                    if interactive.width == 0 || interactive.height == 0 {
                        continue;
                    }
                    let rect = Ui2Rect::new(
                        content.x + interactive.x as f32,
                        content.y + interactive.y as f32,
                        interactive.width as f32,
                        interactive.height as f32,
                    );
                    scene.append(Ui2HitEntry {
                        owner_window_id: self.id,
                        item_id: interactive.item_id,
                        kind: Ui2HitKind::BrowserInteractive,
                        rect,
                        z: self.z,
                    });
                }
            }
            Ui2WindowKind::HostedSurface => {}
        }
    }
}

impl Ui2HitScene {
    fn clear(&mut self) {
        self.entries.clear();
    }

    fn append(&mut self, entry: Ui2HitEntry) {
        self.entries.push(entry);
    }

    fn remove_window(&mut self, owner_window_id: u32) {
        self.entries
            .retain(|entry| entry.owner_window_id != owner_window_id);
    }

    fn hit_at(&self, cursor_x: f32, cursor_y: f32) -> Option<Ui2HitTarget> {
        let mut best: Option<(i16, Ui2HitKind, u32, u32)> = None;
        for entry in &self.entries {
            if !hit_entry_intersects_cursor(entry, cursor_x, cursor_y) {
                continue;
            }
            let candidate = (entry.z, entry.kind, entry.owner_window_id, entry.item_id);
            if best
                .as_ref()
                .map(|current| candidate > *current)
                .unwrap_or(true)
            {
                best = Some(candidate);
            }
        }
        best.map(|(_, kind, owner_window_id, item_id)| Ui2HitTarget {
            owner_window_id,
            item_id,
            kind,
        })
    }
}

fn rebuild_hit_scene(state: &mut Ui2State) {
    let next_seq = state.hit_scene.seq.wrapping_add(1);
    state.last_browser_interactive_seq = hosted_browser_interactive_seq(state);
    let ctx = Ui2HitBuildContext { state };
    let mut next_scene = Ui2HitScene {
        seq: next_seq,
        entries: Vec::new(),
    };
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window_is_renderable(window) {
            continue;
        }
        window.append_hit_entries(&ctx, &mut next_scene);
    }
    state.hit_scene = next_scene;
}

fn refresh_all_window_hit_entries(state: &mut Ui2State) {
    rebuild_hit_scene(state);
}

fn refresh_window_hit_entries(state: &mut Ui2State, owner_window_id: u32) {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == owner_window_id)
        .cloned()
    else {
        state.hit_scene.remove_window(owner_window_id);
        state.hit_scene.seq = state.hit_scene.seq.wrapping_add(1);
        return;
    };

    if window.kind == Ui2WindowKind::HostedBrowser {
        state.last_browser_interactive_seq =
            hosted_interactive_seq(window_browser_instance_id(&window));
    }

    let refreshed_entries = {
        let ctx = Ui2HitBuildContext { state };
        let mut refreshed_scene = Ui2HitScene {
            seq: 0,
            entries: Vec::new(),
        };
        if window_is_renderable(&window) {
            window.append_hit_entries(&ctx, &mut refreshed_scene);
        }
        refreshed_scene.entries
    };

    state.hit_scene.remove_window(owner_window_id);
    state.hit_scene.entries.extend(refreshed_entries);
    state.hit_scene.seq = state.hit_scene.seq.wrapping_add(1);
}

fn refresh_browser_hit_entries_if_needed(state: &mut Ui2State) {
    let next_browser_interactive_seq = hosted_browser_interactive_seq(state);
    if state.last_browser_interactive_seq == next_browser_interactive_seq {
        return;
    }
    let browser_window_ids: Vec<u32> = state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| window.id)
        .collect();
    if browser_window_ids.is_empty() {
        state.last_browser_interactive_seq = next_browser_interactive_seq;
    } else {
        for window_id in browser_window_ids {
            refresh_window_hit_entries(state, window_id);
        }
        state.last_browser_interactive_seq = next_browser_interactive_seq;
    }
}

fn minimized_window_strip_rect(state: &Ui2State, window_id: u32) -> Option<Ui2Rect> {
    let mut slot = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window.visible || window.state != Ui2WindowStateKind::Minimized {
            continue;
        }
        if window.id == window_id {
            let total_w = UI2_MINIMIZED_STRIP_W + UI2_MINIMIZED_STRIP_GAP;
            let cols_f = libm::floorf(
                (((state.view_w as f32) - (UI2_MINIMIZED_STRIP_PAD * 2.0)
                    + UI2_MINIMIZED_STRIP_GAP)
                    / total_w),
            );
            let cols = cols_f.max(1.0) as usize;
            let col = slot % cols;
            let row = slot / cols;
            let x = UI2_MINIMIZED_STRIP_PAD + (col as f32 * total_w);
            let y =
                UI2_MINIMIZED_STRIP_PAD + (row as f32 * (UI2_TITLE_H + UI2_MINIMIZED_STRIP_GAP));
            let max_w = ((state.view_w as f32) - x - UI2_MINIMIZED_STRIP_PAD).max(96.0);
            return Some(Ui2Rect::new(
                x,
                y,
                UI2_MINIMIZED_STRIP_W.min(max_w),
                UI2_TITLE_H,
            ));
        }
        slot = slot.saturating_add(1);
    }
    None
}

fn effective_window_rect(state: &Ui2State, window: &Ui2Window) -> Ui2Rect {
    if window.state == Ui2WindowStateKind::Minimized {
        minimized_window_strip_rect(state, window.id).unwrap_or(Ui2Rect::new(
            UI2_MINIMIZED_STRIP_PAD,
            UI2_MINIMIZED_STRIP_PAD,
            UI2_MINIMIZED_STRIP_W,
            UI2_TITLE_H,
        ))
    } else {
        window.rect
    }
}

fn hit_entry_intersects_cursor(entry: &Ui2HitEntry, cursor_x: f32, cursor_y: f32) -> bool {
    if !rect_contains_point(
        Ui2Rect::new(
            entry.rect.x - UI2_CURSOR_HIT_RADIUS_PX,
            entry.rect.y - UI2_CURSOR_HIT_RADIUS_PX,
            entry.rect.w + (UI2_CURSOR_HIT_RADIUS_PX * 2.0),
            entry.rect.h + (UI2_CURSOR_HIT_RADIUS_PX * 2.0),
        ),
        cursor_x,
        cursor_y,
    ) {
        return false;
    }

    let cursor = Ball::new(UI2_CURSOR_HIT_RADIUS_PX.max(0.5));
    let rect = Cuboid::new(Vector::new(
        (entry.rect.w * 0.5).max(0.5),
        (entry.rect.h * 0.5).max(0.5),
    ));
    let cursor_iso = Isometry::translation(cursor_x, cursor_y);
    let rect_iso = Isometry::translation(
        entry.rect.x + (entry.rect.w * 0.5),
        entry.rect.y + (entry.rect.h * 0.5),
    );
    matches!(
        query::intersection_test(&cursor_iso, &cursor, &rect_iso, &rect),
        Ok(true)
    )
}

fn is_simple_click(press_x: f32, press_y: f32, release_x: f32, release_y: f32) -> bool {
    let dx = release_x - press_x;
    let dy = release_y - press_y;
    let slop_sq = UI2_CLICK_SLOP_PX * UI2_CLICK_SLOP_PX;
    (dx * dx) + (dy * dy) <= slop_sq
}

fn window_info(state: &Ui2State, window: &Ui2Window) -> TrueosUi2WindowInfo {
    let rect = effective_window_rect(state, window);
    let content = window_content_rect(state, window).unwrap_or(Ui2Rect::new(0.0, 0.0, 0.0, 0.0));
    let decoration =
        window_decoration_rect(state, window).unwrap_or(Ui2Rect::new(0.0, 0.0, 0.0, 0.0));
    TrueosUi2WindowInfo {
        id: window.id,
        kind: window_kind_id(window.kind),
        state: window.state as u32,
        decoration_mode: window.decoration_mode as u32,
        icon_id: window.icon_id,
        visible: if window.visible { 1 } else { 0 },
        hit_test_visible: if window.hit_test_visible { 1 } else { 0 },
        selected: if window.selected_cursor_slots.is_empty() {
            0
        } else {
            1
        },
        x: libm::roundf(rect.x) as i32,
        y: libm::roundf(rect.y) as i32,
        width: round_to_u32(rect.w, 0),
        height: round_to_u32(rect.h, 0),
        content_x: libm::roundf(content.x) as i32,
        content_y: libm::roundf(content.y) as i32,
        content_width: round_to_u32(content.w, 0),
        content_height: round_to_u32(content.h, 0),
        decoration_x: libm::roundf(decoration.x) as i32,
        decoration_y: libm::roundf(decoration.y) as i32,
        decoration_width: round_to_u32(decoration.w, 0),
        decoration_height: round_to_u32(decoration.h, 0),
    }
}

fn normalized_window_rect(state: &Ui2State, rect: Ui2Rect) -> Ui2Rect {
    normalized_window_rect_for_view(state.view_w, state.view_h, rect)
}

fn normalized_window_rect_for_view(view_w: u32, view_h: u32, rect: Ui2Rect) -> Ui2Rect {
    let min_extent = ui2_window_min_extent();
    let max_w = (view_w as f32).max(min_extent);
    let max_h = (view_h as f32).max(min_extent);
    Ui2Rect::new(
        rect.x,
        rect.y,
        rect.w.max(min_extent).min(max_w),
        rect.h.max(min_extent).min(max_h),
    )
}

fn save_window_restore_rect(state: &Ui2State, window: &mut Ui2Window) {
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = normalized_window_rect(state, window.rect);
    }
}

fn maximize_window_rect(state: &Ui2State) -> Ui2Rect {
    Ui2Rect::new(
        0.0,
        0.0,
        (state.view_w as f32).max(1.0),
        (state.view_h as f32).max(1.0),
    )
}

fn left_half_window_rect(state: &Ui2State) -> Ui2Rect {
    let view_w = (state.view_w as f32).max(1.0);
    let view_h = (state.view_h as f32).max(1.0);
    Ui2Rect::new(0.0, 0.0, (view_w * 0.5).max(1.0), view_h)
}

fn right_half_window_rect(state: &Ui2State) -> Ui2Rect {
    let view_w = (state.view_w as f32).max(1.0);
    let view_h = (state.view_h as f32).max(1.0);
    let half_w = (view_w * 0.5).max(1.0);
    Ui2Rect::new(view_w - half_w, 0.0, half_w, view_h)
}

fn set_window_rect_in_state(
    state: &mut Ui2State,
    id: u32,
    rect: Ui2Rect,
    reason: &'static str,
) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    let next_rect = normalized_window_rect_for_view(view_w, view_h, rect);
    if window.state == Ui2WindowStateKind::Normal && window.rect == next_rect {
        return true;
    }
    if window.state != Ui2WindowStateKind::Normal {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.rect = next_rect;
    window.restore_rect = next_rect;
    window.state = Ui2WindowStateKind::Normal;
    state.compose_reason = reason;
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, reason)
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

fn commit_window_geometry_change(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let noted = note_window_dirty(state, id, reason);
    if noted {
        let _ = note_window_viewport_sync_needed(state, id);
        refresh_window_hit_entries(state, id);
    }
    noted
}

fn minimize_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Minimized {
        return true;
    }
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.state = Ui2WindowStateKind::Minimized;
    state.compose_reason = "minimize-window";
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, "minimize-window")
}

fn maximize_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let next_rect = maximize_window_rect(state);
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Maximized && window.rect == next_rect {
        return true;
    }
    if window.state != Ui2WindowStateKind::Maximized {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.rect = next_rect;
    window.state = Ui2WindowStateKind::Maximized;
    state.compose_reason = "maximize-window";
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, "maximize-window")
}

fn restore_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Normal {
        return true;
    }
    if window.restore_rect.w > 0.0 && window.restore_rect.h > 0.0 {
        window.rect = normalized_window_rect_for_view(view_w, view_h, window.restore_rect);
    }
    window.state = Ui2WindowStateKind::Normal;
    state.compose_reason = "restore-window";
    commit_window_geometry_change(state, id, "restore-window")
}

fn set_window_visible_in_state(state: &mut Ui2State, id: u32, visible: bool) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.visible = visible;
    let reason = if visible {
        "show-window"
    } else {
        "hide-window"
    };
    state.compose_reason = reason;
    if !visible {
        state.hit_scene.remove_window(id);
        state.hit_scene.seq = state.hit_scene.seq.wrapping_add(1);
        clear_window_drag_claims(state, id);
    }
    let noted = note_window_dirty(state, id, reason);
    if noted {
        let _ = note_window_viewport_sync_needed(state, id);
        refresh_window_hit_entries(state, id);
    }
    noted
}

fn handle_system_button_action(
    state: &mut Ui2State,
    window_id: u32,
    action: Ui2SystemButtonAction,
) -> bool {
    match action {
        Ui2SystemButtonAction::Fork => fork_window_in_state(state, window_id),
        Ui2SystemButtonAction::Minimize => minimize_window_in_state(state, window_id),
        Ui2SystemButtonAction::ToggleMaximize => {
            let is_maximized = state
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.state == Ui2WindowStateKind::Maximized)
                .unwrap_or(false);
            if is_maximized {
                restore_window_in_state(state, window_id)
            } else {
                maximize_window_in_state(state, window_id)
            }
        }
        Ui2SystemButtonAction::Close => set_window_visible_in_state(state, window_id, false),
    }
}

fn fork_window_in_state(state: &mut Ui2State, source_window_id: u32) -> bool {
    let Some(source_window) = state
        .windows
        .iter()
        .find(|window| window.id == source_window_id)
    else {
        return false;
    };

    let source_rect = if source_window.state == Ui2WindowStateKind::Normal {
        source_window.rect
    } else if source_window.restore_rect.w > 0.0 && source_window.restore_rect.h > 0.0 {
        source_window.restore_rect
    } else {
        source_window.rect
    };
    let next_rect = normalized_window_rect_for_view(
        state.view_w,
        state.view_h,
        Ui2Rect::new(
            source_rect.x + UI2_BROWSER_FORK_WINDOW_OFFSET_PX,
            source_rect.y + UI2_BROWSER_FORK_WINDOW_OFFSET_PX,
            source_rect.w,
            source_rect.h,
        ),
    );
    let next_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(source_window.z)
        .saturating_add(1);
    let next_title = source_window.title.clone();
    let next_icon_id = source_window.icon_id;
    let next_alpha = source_window.alpha;
    let next_hit_test_visible = source_window.hit_test_visible;
    let next_decoration_mode = source_window.decoration_mode;
    let next_titlebar_visible = source_window.titlebar_visible;
    let next_bottom_bar_visible = source_window.bottom_bar_visible;
    let next_left_scrollbar_visible = source_window.left_scrollbar_visible;
    let next_bottom_scrollbar_visible = source_window.bottom_scrollbar_visible;
    let next_vertical_scrollbar_side = source_window.vertical_scrollbar_side;
    let next_horizontal_scrollbar_side = source_window.horizontal_scrollbar_side;
    let next_content_tex_blend = source_window.content_tex_blend;
    let next_kind = source_window.kind;

    let (next_browser_instance_id, next_tex_id, fork_reason) = match next_kind {
        Ui2WindowKind::HostedBrowser => {
            let source_browser_instance_id = window_browser_instance_id(source_window);
            let target_browser_instance_id = trueos_qjs::browser_task::BOOT_BROWSER_INSTANCE_IDS
                .iter()
                .copied()
                .find(|browser_instance_id| {
                    *browser_instance_id != source_browser_instance_id
                        && state.windows.iter().all(|window| {
                            window.kind != Ui2WindowKind::HostedBrowser
                                || window_browser_instance_id(window) != *browser_instance_id
                        })
                });
            let Some(target_browser_instance_id) = target_browser_instance_id else {
                crate::log!(
                    "ui2: browser-fork no-target window={} source_browser={}\n",
                    source_window_id,
                    source_browser_instance_id
                );
                return false;
            };
            (
                target_browser_instance_id,
                trueos_qjs::browser_task::render_tex_id_for_browser_instance(
                    target_browser_instance_id,
                ),
                "fork-browser-window",
            )
        }
        Ui2WindowKind::HostedSurface => (0, source_window.content_tex_id, "fork-surface-window"),
    };

    let id = alloc_window(
        state,
        next_kind,
        next_title.as_str(),
        next_rect,
        next_z,
        next_alpha,
    );
    if let Some(window) = window_mut(state, id) {
        window.browser_instance_id = next_browser_instance_id;
        window.icon_id = next_icon_id;
        window.content_tex_id = next_tex_id;
        window.content_tex_blend = next_content_tex_blend;
        window.hit_test_visible = next_hit_test_visible;
        window.decoration_mode = next_decoration_mode;
        window.titlebar_visible = next_titlebar_visible;
        window.bottom_bar_visible = next_bottom_bar_visible;
        window.left_scrollbar_visible = next_left_scrollbar_visible;
        window.bottom_scrollbar_visible = next_bottom_scrollbar_visible;
        window.vertical_scrollbar_side = next_vertical_scrollbar_side;
        window.horizontal_scrollbar_side = next_horizontal_scrollbar_side;
        window.state = Ui2WindowStateKind::Normal;
        window.rect = next_rect;
        window.restore_rect = next_rect;
    }

    let initial_content = state
        .windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window_content_rect(state, window))
        .map(|content| {
            let (_, _, width, height) = snap_browser_content_rect(content);
            (width, height)
        });

    if next_kind == Ui2WindowKind::HostedBrowser {
        let _ = hosted_bind_window(next_browser_instance_id, id);
        let _ = trueos_qjs::browser_task::set_browser_render_target_tex_id_for_browser(
            next_browser_instance_id,
            next_tex_id,
        );
    }
    state.compose_reason = fork_reason;
    let _ = note_window_dirty(state, id, fork_reason);
    let _ = note_window_viewport_sync_needed(state, id);
    refresh_window_hit_entries(state, id);
    match next_kind {
        Ui2WindowKind::HostedBrowser => {
            crate::log!(
                "ui2: browser-fork window={} browser={} from_window={}\n",
                id,
                next_browser_instance_id,
                source_window_id
            );
        }
        Ui2WindowKind::HostedSurface => {
            crate::log!(
                "ui2: surface-fork window={} tex={} from_window={}\n",
                id,
                next_tex_id,
                source_window_id
            );
        }
    }

    if let Some((width, height)) = initial_content
        && next_kind == Ui2WindowKind::HostedBrowser
    {
        let pixels =
            alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
        let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            next_tex_id,
            width,
            height,
            pixels.as_slice(),
            id,
            fork_reason,
        );
    }

    true
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
    true
}

fn ensure_window_texture_size(
    tex_id: u32,
    width: u32,
    height: u32,
    repaint_window_id: u32,
    repaint_reason: &'static str,
) -> bool {
    if tex_id == 0 || width == 0 || height == 0 {
        return false;
    }

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

    let pixels =
        alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
    crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        tex_id,
        width,
        height,
        pixels.as_slice(),
        repaint_window_id,
        repaint_reason,
    )
}

fn sync_window_container(
    window_id: u32,
    renderable: bool,
    kind: Ui2WindowKind,
    content_id: HostedContentId,
    content_tex_id: u32,
    content: Option<Ui2Rect>,
) -> bool {
    if !renderable {
        return true;
    }
    match kind {
        Ui2WindowKind::HostedBrowser => {
            let Some(content) = content else {
                return true;
            };
            let (_, _, viewport_w, viewport_h) = snap_browser_content_rect(content);
            if !ensure_window_texture_size(
                content_tex_id,
                viewport_w,
                viewport_h,
                window_id,
                "browser-tab-texture-resize",
            ) {
                return false;
            }
            queue_browser_window_viewport(content_id, content)
        }
        Ui2WindowKind::HostedSurface => true,
    }
}

fn sync_pending_window_containers(state: &mut Ui2State) {
    let pending: Vec<(
        u32,
        bool,
        Ui2WindowKind,
        HostedContentId,
        u32,
        Option<Ui2Rect>,
    )> = state
        .windows
        .iter()
        .filter(|window| window.container_sync_needed)
        .map(|window| {
            let renderable = window_is_renderable(window);
            let content = if renderable {
                window_content_rect(state, window)
            } else {
                None
            };
            (
                window.id,
                renderable,
                window.kind,
                window_browser_instance_id(window),
                window.content_tex_id,
                content,
            )
        })
        .collect();

    let mut synced_ids = Vec::new();
    for (id, renderable, kind, content_id, content_tex_id, content) in pending {
        if sync_window_container(id, renderable, kind, content_id, content_tex_id, content) {
            synced_ids.push(id);
        }
    }
    for id in synced_ids {
        if let Some(window) = window_mut(state, id) {
            window.container_sync_needed = false;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_primary_browser_window_id() -> u32 {
    browser_window_id().unwrap_or(0)
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

fn draw_window_system_button(state: &Ui2State, window: &Ui2Window, action: Ui2SystemButtonAction) {
    if window.state == Ui2WindowStateKind::Minimized
        && action != Ui2SystemButtonAction::ToggleMaximize
    {
        return;
    }
    let Some(rect) = window_system_button_rect(state, window, action) else {
        return;
    };

    match action {
        Ui2SystemButtonAction::Fork => {
            let text = b"+1";
            let text_w = crate::gfx::imba_athlas::imba_athlas_text_width_px(text);
            crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
                text,
                rect.x + ((rect.w - text_w) * 0.5),
                rect.y + 3.0,
                state.view_w,
                state.view_h,
                window.alpha,
            );
        }
        Ui2SystemButtonAction::Close => {
            let text = b"-1";
            let text_w = crate::gfx::imba_athlas::imba_athlas_text_width_px(text);
            crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
                text,
                rect.x + ((rect.w - text_w) * 0.5),
                rect.y + 3.0,
                state.view_w,
                state.view_h,
                window.alpha,
            );
        }
        Ui2SystemButtonAction::Minimize | Ui2SystemButtonAction::ToggleMaximize => {
            let icon_id = match action {
                Ui2SystemButtonAction::Fork => 0,
                Ui2SystemButtonAction::Minimize => 5,
                Ui2SystemButtonAction::ToggleMaximize => 7,
                Ui2SystemButtonAction::Close => 0,
            };
            let icon_x = rect.x + ((rect.w - 16.0) * 0.5);
            let icon_y = rect.y + ((rect.h - 16.0) * 0.5);
            let _ = crate::gfx::lyon::draw_lyon_icon_alpha_no_present(
                icon_id,
                0,
                1,
                icon_x,
                icon_y,
                state.view_w,
                state.view_h,
                window.alpha,
            );
        }
    }
}

fn draw_window_bottom_resize_button(state: &Ui2State, window: &Ui2Window) {
    let Some(rect) = window_bottom_resize_button_rect(state, window) else {
        return;
    };
    let icon_side = 16.0f32;
    let icon_x = rect.x + ((rect.w - icon_side) * 0.5);
    let icon_y = rect.y + ((rect.h - icon_side) * 0.5);
    let _ = crate::gfx::lyon::draw_lyon_icon_alpha_no_present(
        1,
        0,
        1,
        icon_x,
        icon_y,
        state.view_w,
        state.view_h,
        window.alpha,
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

fn draw_hosted_browser_text_rows(state: &Ui2State, window: &Ui2Window, content: Ui2Rect) -> bool {
    let text_state = hosted_text_state(window_browser_instance_id(window));
    if text_state.rows.is_empty() {
        return false;
    }

    let panel_rgba = modulate_rgba_alpha((0xF8, 0xF8, 0xF4, 0xFF), window.alpha);
    let row_rgba = modulate_rgba_alpha((0xDC, 0xE3, 0xED, 0xFF), window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    for (idx, row) in text_state.rows.iter().take(10).enumerate() {
        let top = content.y + 10.0 + (idx as f32 * 18.0);
        if top + 18.0 > content.y + content.h {
            break;
        }
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            content.x + 6.0,
            top - 1.0,
            (content.w - 12.0).max(1.0),
            16.0,
            row_rgba,
            state.view_w,
            state.view_h,
        );
        crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
            row.text.as_bytes(),
            content.x + 10.0 + row.indent_px as f32,
            top + 1.0,
            state.view_w,
            state.view_h,
            window.alpha,
        );
    }

    true
}

fn draw_window_frame(state: &Ui2State, window: &Ui2Window) {
    if !window_is_renderable(window) {
        return;
    }

    let rect = effective_window_rect(state, window);
    let content_rect = window_content_rect(state, window);
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
        .map(|slot_id| ui2_hid::cursor_color(*slot_id))
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
            0.5,
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
            let _ = crate::gfx::lyon::draw_lyon_icon_alpha_no_present(
                window.icon_id,
                0,
                1,
                icon_x,
                icon_y,
                state.view_w,
                state.view_h,
                window.alpha,
            );
        }
        let title_text_h = (UI2_TITLE_H - 2.0).max(1.0);
        let title_w = crate::gfx::imbafont::measure_text_width_px(
            crate::gfx::imbafont::ImbaFontFace::Impact,
            window.title.as_bytes(),
            title_text_h,
        );
        let title_left = if window.icon_id != 0 {
            rect.x + 28.0
        } else {
            rect.x + 8.0
        };
        let title_x = (rect.x + ((rect.w - title_w) * 0.5)).max(title_left);
        let title_y = rect.y + ((UI2_TITLE_H - title_text_h) * 0.5);
        if let Some(layout) = crate::gfx::imbafont::layout_text_top_left(
            crate::gfx::imbafont::ImbaFontFace::Impact,
            window.title.as_bytes(),
            title_x,
            title_y,
            title_text_h,
        ) {
            let _ = crate::gfx::imbafont::draw_text_in_frame(
                crate::gfx::imbafont::ImbaFontFace::Impact,
                window.title.as_bytes(),
                &layout,
                state.view_w,
                state.view_h,
                (title_rgba.0, title_rgba.1, title_rgba.2),
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

    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            if let Some(content) = content_rect {
                if draw_hosted_browser_text_rows(state, window, content) {
                    return;
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
                    return;
                }
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Starting Browser Tab",
                    b"Waiting for texture frame",
                );
                return;
            }
        }
        Ui2WindowKind::HostedSurface => {
            if let Some(content) = content_rect {
                if texture_is_drawable(window.content_tex_id)
                    && draw_texture_rect_no_present(
                        window.content_tex_id,
                        content.x,
                        content.y,
                        content.w,
                        content.h,
                        state.view_w,
                        state.view_h,
                        window.content_tex_blend,
                        window.alpha,
                    )
                {
                    return;
                }
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Preparing Window",
                    b"Waiting for texture upload",
                );
                return;
            }
        }
    }
}

fn update_compose_fps_history(state: &mut Ui2State, now_ms: u64) {
    state.compose_present_history_ms.push(now_ms);
    let window_start = now_ms.saturating_sub(1000);
    state
        .compose_present_history_ms
        .retain(|&sample_ms| sample_ms >= window_start);
    state.compose_fps_display = state.compose_present_history_ms.len().min(999) as u16;
}

fn draw_compose_fps_overlay(state: &Ui2State) {
    let text = format!("{:03}", state.compose_fps_display.min(999));
    let text_w = crate::gfx::imba_athlas::imba_athlas_text_width_px(text.as_bytes());
    let pad_x = 6.0f32;
    let pad_y = 4.0f32;
    let box_w = text_w + pad_x * 2.0;
    let box_h = 16.0f32;
    let x = ((state.view_w as f32) - box_w - 8.0).max(0.0);
    let y = ((state.view_h as f32) - box_h - 6.0).max(0.0);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y,
        box_w,
        box_h,
        (0xF8, 0xF8, 0xF4, 0xD8),
        state.view_w,
        state.view_h,
    );
    crate::gfx::imba_athlas::draw_imba_athlas_text_in_frame_alpha(
        text.as_bytes(),
        x + pad_x,
        y + pad_y,
        state.view_w,
        state.view_h,
        255,
    );
}

fn compose_windows(state: &mut Ui2State) {
    let dirty_count = state.windows.iter().filter(|window| window.dirty).count();
    let presentation_allowed = crate::r::readiness::is_set(crate::r::readiness::LOADSCREEN_END);
    for window in &mut state.windows {
        if window.dirty {
            window.dirty_seq = window.dirty_seq.wrapping_add(1);
            let repeated_reason = window.last_logged_reason == window.last_reason;
            let since_last_log = window.dirty_seq.wrapping_sub(window.last_logged_dirty_seq);
            if window.last_logged_dirty_seq == 0
                || !repeated_reason
                || since_last_log >= UI2_WINDOW_UPDATE_LOG_EVERY
            {
                let suppressed = if repeated_reason {
                    since_last_log.saturating_sub(1)
                } else {
                    0
                };
                if crate::logflag::UI2_ENABLE_VERBOSE_COMPOSE_LOGS {
                    crate::log!(
                        "ui2: window-update id={} seq={} reason={} suppressed={}\n",
                        window.id,
                        window.dirty_seq,
                        window.last_reason,
                        suppressed
                    );
                }
                window.last_logged_dirty_seq = window.dirty_seq;
                window.last_logged_reason = window.last_reason;
            }
            window.dirty = false;
        }
    }

    state.compose_seq = state.compose_seq.wrapping_add(1);
    let repeated_reason = state.last_logged_compose_reason == state.compose_reason;
    let since_last_log = state
        .compose_seq
        .wrapping_sub(state.last_logged_compose_seq);
    if state.last_logged_compose_seq == 0
        || !repeated_reason
        || dirty_count != state.last_logged_compose_dirty_count
        || since_last_log >= UI2_COMPOSE_LOG_EVERY
    {
        let suppressed = if repeated_reason && dirty_count == state.last_logged_compose_dirty_count
        {
            since_last_log.saturating_sub(1)
        } else {
            0
        };
        if crate::logflag::UI2_ENABLE_VERBOSE_COMPOSE_LOGS {
            crate::log!(
                "ui2: compose seq={} windows={} dirty={} reason={} suppressed={}\n",
                state.compose_seq,
                state.windows.len(),
                dirty_count,
                state.compose_reason,
                suppressed
            );
        }
        state.last_logged_compose_seq = state.compose_seq;
        state.last_logged_compose_reason = state.compose_reason;
        state.last_logged_compose_dirty_count = dirty_count;
    }

    if presentation_allowed {
        crate::gfx::with_cabi_frame_lock(|| {
            if state.compose_seq <= 2 {
                crate::log!(
                    "ui2: compose-frame seq={} begin windows={} dirty={}\n",
                    state.compose_seq,
                    state.windows.len(),
                    dirty_count
                );
            }
            let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
            if begin_rc != 0 {
                return;
            }
            for idx in sorted_window_indices(state) {
                let window = &state.windows[idx];
                draw_window_frame(state, window);
            }
            draw_resize_preview_outline(state);
            draw_compose_fps_overlay(state);
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
            update_compose_fps_history(state, boot_probe_ms());
            if state.compose_seq <= 2 {
                crate::log!("ui2: compose-frame seq={} end\n", state.compose_seq);
            }
        });
    } else if state.compose_seq <= 2 {
        crate::log!(
            "ui2: compose-frame seq={} deferred-by-loadscreen dirty={}\n",
            state.compose_seq,
            dirty_count
        );
    }

    if !state.first_compose_signaled {
        crate::r::readiness::set(crate::r::readiness::UI2_READY);
        state.first_compose_signaled = true;
    }
}

#[embassy_executor::task]
pub async fn ui2_task() {
    if UI2_STARTED.swap(true, Ordering::SeqCst) {
        crate::log!("ui2: already running\n");
        return;
    }

    crate::log!("boot-probe: ui2 task start ms={}\n", boot_probe_ms());
    crate::gfx::init(crate::limine::framebuffer_response());
    init_state();
    request_full_recompose("boot");
    crate::log!("ui2: boot window manager\n");
    let mut last_browser_content_seq = 0u32;
    let mut loop_seq = 0u32;

    loop {
        loop_seq = loop_seq.wrapping_add(1);
        if loop_seq <= 4 {
            crate::log!("ui2: loop seq={} begin\n", loop_seq);
        }
        if let Some(active_mask) = take_hosted_browser_factory_mask() {
            let created = sync_hosted_browser_factory_windows(active_mask);
            if created != 0 {
                crate::log!(
                    "ui2: hosted-browser-factory reconcile mask={:#x} created={}\n",
                    active_mask,
                    created
                );
            }
        }
        let mut browser_content_changed = false;
        {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            if loop_seq <= 4 {
                crate::log!("ui2: loop seq={} locked\n", loop_seq);
            }
            refresh_browser_hit_entries_if_needed(&mut state);
            ui2_hid::pump_cursor_selection(&mut state);
            ui2_hid::pump_keyboard_input(&mut state);
            sync_pending_window_containers(&mut state);
            let next_browser_content_seq = hosted_browser_content_seq(&state);
            if next_browser_content_seq != last_browser_content_seq {
                last_browser_content_seq = next_browser_content_seq;
                log_browser_surface_updates(&mut state);
                browser_content_changed = true;
            }
        }
        if browser_content_changed {
            request_full_recompose("browser-content");
        }
        let dirty = UI2_DIRTY.swap(false, Ordering::AcqRel);
        if loop_seq <= 4 {
            crate::log!(
                "ui2: loop seq={} dirty={} browser_content_changed={}\n",
                loop_seq,
                dirty,
                browser_content_changed
            );
        }
        if dirty {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            if crate::logflag::GFX_FRAME_PROGRESS_LOGS && loop_seq <= 4 {
                crate::log!("ui2: compose seq={} start\n", loop_seq);
            }
            if state.compose_seq == 0 {
                crate::log!(
                    "boot-probe: ui2 first compose begin ms={}\n",
                    boot_probe_ms()
                );
            }
            compose_windows(&mut state);
            if state.first_compose_signaled && state.compose_seq == 1 {
                crate::log!("boot-probe: ui2 ready ms={}\n", boot_probe_ms());
            }
            if crate::logflag::GFX_FRAME_PROGRESS_LOGS && loop_seq <= 4 {
                crate::log!("ui2: compose seq={} done\n", loop_seq);
            }
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
