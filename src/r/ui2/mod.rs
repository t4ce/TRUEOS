#![allow(dead_code)]

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

use self::ui2_hosted::*;
pub use self::ui2_win::*;
pub use self::ui2_win_deco::*;

const UI2_TITLE_H: f32 = 26.0;
const UI2_BOTTOM_BAR_H: f32 = 18.0;
const UI2_SYSTEM_SCROLLBAR_PX: f32 = 4.0;
const UI2_SYSTEM_BUTTON_W: f32 = 24.0;
const UI2_SYSTEM_BUTTON_H: f32 = 14.0;
const UI2_SYSTEM_BUTTON_GAP: f32 = 4.0;
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
const UI2_STATIC_BROWSER_VIEWPORT_W: u32 = 512;
const UI2_STATIC_BROWSER_VIEWPORT_H: u32 = 512;

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
    last_logged_browser_surface_seq: u32,
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
    move_drag: Ui2WindowMoveDrag,
    resize_drag: Ui2WindowResizeDrag,
    scroll_drag: Ui2WindowScrollDrag,
    scroll_pan_drag: Ui2WindowScrollPanDrag,
    windows: Vec<Ui2Window>,
    loadscreen_end_signaled: bool,
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
            move_drag: Ui2WindowMoveDrag::default(),
            resize_drag: Ui2WindowResizeDrag::default(),
            scroll_drag: Ui2WindowScrollDrag::default(),
            scroll_pan_drag: Ui2WindowScrollPanDrag::default(),
            windows: Vec::new(),
            loadscreen_end_signaled: false,
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
        last_logged_browser_surface_seq: 0,
    });
    id
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
    if snapshot.viewport_width == 0 {
        snapshot.viewport_width = UI2_STATIC_BROWSER_VIEWPORT_W;
    }
    if snapshot.viewport_height == 0 {
        snapshot.viewport_height = UI2_STATIC_BROWSER_VIEWPORT_H;
    }
    if snapshot.content_width == 0 {
        snapshot.content_width = snapshot.viewport_width;
    }
    if snapshot.content_height == 0 {
        snapshot.content_height = snapshot.viewport_height;
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

fn hosted_browser_surface_seq(state: &Ui2State) -> u32 {
    state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| {
            let instance_id = window_browser_instance_id(window);
            let seq = hosted_surface_seq(instance_id);
            instance_id.wrapping_mul(1315423911).wrapping_add(seq)
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
    if state.move_drag.window_id == id {
        state.move_drag = Ui2WindowMoveDrag::default();
    }
    if state.resize_drag.window_id == id {
        state.resize_drag = Ui2WindowResizeDrag::default();
    }
    if state.scroll_drag.window_id == id {
        state.scroll_drag = Ui2WindowScrollDrag::default();
    }
    if state.scroll_pan_drag.window_id == id {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    }
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
    if state.move_drag.window_id == id {
        state.move_drag = Ui2WindowMoveDrag::default();
    }
    if state.resize_drag.window_id == id {
        state.resize_drag = Ui2WindowResizeDrag::default();
    }
    if state.scroll_drag.window_id == id {
        state.scroll_drag = Ui2WindowScrollDrag::default();
    }
    if state.scroll_pan_drag.window_id == id {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    }
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
    if state.move_drag.window_id == id {
        state.move_drag = Ui2WindowMoveDrag::default();
    }
    if state.resize_drag.window_id == id {
        state.resize_drag = Ui2WindowResizeDrag::default();
    }
    if state.scroll_drag.window_id == id {
        state.scroll_drag = Ui2WindowScrollDrag::default();
    }
    if state.scroll_pan_drag.window_id == id {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    }
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
        if state.move_drag.window_id == id {
            state.move_drag = Ui2WindowMoveDrag::default();
        }
        if state.resize_drag.window_id == id {
            state.resize_drag = Ui2WindowResizeDrag::default();
        }
        if state.scroll_drag.window_id == id {
            state.scroll_drag = Ui2WindowScrollDrag::default();
        }
        if state.scroll_pan_drag.window_id == id {
            state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
        }
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
    let drag = state.resize_drag;
    if !drag.active || drag.live_apply {
        return;
    }
    let rect = drag.preview_rect;
    if !(rect.w > 0.0 && rect.h > 0.0) {
        return;
    }
    let outer = (0x2B, 0x6C, 0xD6, 0xFF);
    let inner = (0xD9, 0xE7, 0xFF, 0xFF);
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

fn sync_window_container(
    renderable: bool,
    kind: Ui2WindowKind,
    content_id: HostedContentId,
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
            queue_browser_window_viewport(content_id, content)
        }
        Ui2WindowKind::HostedSurface => true,
    }
}

fn sync_pending_window_containers(state: &mut Ui2State) {
    let pending: Vec<(u32, bool, Ui2WindowKind, HostedContentId, Option<Ui2Rect>)> = state
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
                content,
            )
        })
        .collect();

    let mut synced_ids = Vec::new();
    for (id, renderable, kind, content_id, content) in pending {
        if sync_window_container(renderable, kind, content_id, content) {
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

const HOSTED_SCENE_CMD_DRAW_TEX_RECT_UV: u8 = 1;

#[inline]
fn hosted_scene_cmd_read_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let slice = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[inline]
fn hosted_scene_cmd_read_f32(bytes: &[u8], cursor: &mut usize) -> Option<f32> {
    hosted_scene_cmd_read_u32(bytes, cursor).map(f32::from_bits)
}

fn draw_hosted_scene_cmds_no_present(
    state: &Ui2State,
    window: &Ui2Window,
    snapshot: &UiHostedSurfaceState,
    draw_x: f32,
    draw_y: f32,
) -> bool {
    let bytes = snapshot.scene_cmds.as_slice();
    if bytes.is_empty() {
        return false;
    }

    let mut cursor = 0usize;
    let mut drew = false;
    while cursor < bytes.len() {
        let op = bytes[cursor];
        cursor += 1;
        match op {
            HOSTED_SCENE_CMD_DRAW_TEX_RECT_UV => {
                let Some(tex_id) = hosted_scene_cmd_read_u32(bytes, &mut cursor) else {
                    break;
                };
                let Some(x) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(y) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(width) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(height) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(u0) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(v0) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(u1) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(v1) = hosted_scene_cmd_read_f32(bytes, &mut cursor) else {
                    break;
                };
                let Some(&blend_u8) = bytes.get(cursor) else {
                    break;
                };
                cursor += 1;
                let Some(&cmd_alpha) = bytes.get(cursor) else {
                    break;
                };
                cursor += 1;

                if !texture_is_drawable(tex_id) {
                    continue;
                }
                let alpha = modulate_alpha(cmd_alpha, window.alpha);
                drew |= draw_texture_rect_uv_no_present(
                    tex_id,
                    draw_x + x,
                    draw_y + y,
                    width,
                    height,
                    u0,
                    v0,
                    u1,
                    v1,
                    state.view_w,
                    state.view_h,
                    blend_u8 != 0,
                    alpha,
                );
            }
            _ => break,
        }
    }

    drew
}

fn hosted_browser_has_drawable_content(snapshot: &UiHostedSurfaceState) -> bool {
    if snapshot.viewport_width == 0 || snapshot.viewport_height == 0 {
        return false;
    }
    !snapshot.scene_cmds.is_empty()
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

    let headline_w = crate::gfx::text::atlas_text_width_px(headline);
    let headline_x = content.x + ((content.w - headline_w) * 0.5).max(10.0);
    let headline_y = content.y + 40.0;
    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        headline,
        headline_x,
        headline_y,
        state.view_w,
        state.view_h,
        headline_alpha,
    );

    let subline_w = crate::gfx::text::atlas_text_width_px(subline);
    let subline_x = content.x + ((content.w - subline_w) * 0.5).max(10.0);
    let subline_y = headline_y + 20.0;
    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        subline,
        subline_x,
        subline_y,
        state.view_w,
        state.view_h,
        subline_alpha,
    );
}

fn draw_browser_window_content(state: &Ui2State, window: &Ui2Window, content: Ui2Rect) -> bool {
    let snapshot = browser_surface_state_for_window(window);
    if !hosted_browser_has_drawable_content(&snapshot) {
        return false;
    }

    let (draw_x_i, draw_y_i, snapped_w, snapped_h) = snap_browser_content_rect(content);
    let sx = draw_x_i.max(0) as u32;
    let sy = draw_y_i.max(0) as u32;
    let sw = snapped_w;
    let sh = snapped_h;
    let draw_x_f = draw_x_i as f32;
    let draw_y_f = draw_y_i as f32;
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_scissor(sx, sy, sw, sh) };

    let drew = draw_hosted_scene_cmds_no_present(state, window, &snapshot, draw_x_f, draw_y_f);

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_clear_scissor() };
    drew
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

    let icon_id = match action {
        Ui2SystemButtonAction::Minimize => 5,
        Ui2SystemButtonAction::ToggleMaximize => 7,
        Ui2SystemButtonAction::Close => 11,
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
        let title_w = crate::gfx::text::atlas_text_width_px(window.title.as_bytes());
        let title_left = if window.icon_id != 0 {
            rect.x + 28.0
        } else {
            rect.x + 8.0
        };
        let title_x = (rect.x + ((rect.w - title_w) * 0.5)).max(title_left);
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            window.title.as_bytes(),
            title_x,
            rect.y + 5.0,
            state.view_w,
            state.view_h,
            title_rgba.3,
        );
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
                if draw_browser_window_content(state, window, content) {
                    return;
                }
                draw_window_content_placeholder(
                    state,
                    window,
                    content,
                    b"Starting Browser",
                    b"Waiting for first frame",
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

fn compose_windows(state: &mut Ui2State) {
    let dirty_count = state.windows.iter().filter(|window| window.dirty).count();
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

    crate::gfx::with_cabi_frame_lock(|| {
        if state.compose_seq <= 2 {
            crate::log!(
                "ui2: compose-frame seq={} begin windows={} dirty={}\n",
                state.compose_seq,
                state.windows.len(),
                dirty_count
            );
        }
        unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
        for idx in sorted_window_indices(state) {
            let window = &state.windows[idx];
            draw_window_frame(state, window);
        }
        draw_resize_preview_outline(state);
        unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
        if state.compose_seq <= 2 {
            crate::log!("ui2: compose-frame seq={} end\n", state.compose_seq);
        }
    });

    if !state.loadscreen_end_signaled {
        crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
        state.loadscreen_end_signaled = true;
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
    if let Some(state_lock) = UI2_STATE.get() {
        let mut state = state_lock.lock();
        if !state.loadscreen_end_signaled {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            state.loadscreen_end_signaled = true;
            crate::log!(
                "boot-probe: ui2 signaled loadscreen_end ms={}\n",
                boot_probe_ms()
            );
        }
    }
    crate::log!("ui2: boot window manager\n");
    let mut last_browser_surface_seq = 0u32;
    let mut loop_seq = 0u32;

    loop {
        loop_seq = loop_seq.wrapping_add(1);
        if loop_seq <= 4 {
            crate::log!("ui2: loop seq={} begin\n", loop_seq);
        }
        let mut browser_surface_changed = false;
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
            let next_browser_surface_seq = hosted_browser_surface_seq(&state);
            if next_browser_surface_seq != last_browser_surface_seq {
                last_browser_surface_seq = next_browser_surface_seq;
                log_browser_surface_updates(&mut state);
                browser_surface_changed = true;
            }
        }
        if browser_surface_changed {
            request_full_recompose("browser-surface");
        }
        let dirty = UI2_DIRTY.swap(false, Ordering::AcqRel);
        if loop_seq <= 4 {
            crate::log!(
                "ui2: loop seq={} dirty={} browser_surface_changed={}\n",
                loop_seq,
                dirty,
                browser_surface_changed
            );
        }
        if dirty {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            if loop_seq <= 4 {
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
            if loop_seq <= 4 {
                crate::log!("ui2: compose seq={} done\n", loop_seq);
            }
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
