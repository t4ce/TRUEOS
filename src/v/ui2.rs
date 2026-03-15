use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use parry2d::math::{Isometry, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};
use spin::{Mutex, Once};

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
const UI2_WINDOW_EDGE_DROP_PX: f32 = 8.0;
const UI2_WHEEL_SCROLL_STEP_PX: i32 = 16;
const UI2_WINDOW_UPDATE_LOG_EVERY: u32 = 32;
const UI2_COMPOSE_LOG_EVERY: u32 = 32;
const UI2_ENABLE_VERBOSE_COMPOSE_LOGS: bool = false;
const UI2_WINDOW_RESIZE_LEFT: u32 = 1 << 0;
const UI2_WINDOW_RESIZE_TOP: u32 = 1 << 1;
const UI2_WINDOW_RESIZE_RIGHT: u32 = 1 << 2;
const UI2_WINDOW_RESIZE_BOTTOM: u32 = 1 << 3;

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

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowDecorationMode {
    System = 0,
    Client = 1,
    None = 2,
}

impl Ui2WindowDecorationMode {
    #[inline]
    const fn from_u32(value: u32) -> Option<Self> {
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
    const fn from_u32(value: u32) -> Option<Self> {
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
    const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Top),
            1 => Some(Self::Bottom),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowStateKind {
    Normal = 0,
    Minimized = 1,
    Maximized = 2,
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
    rect: Ui2Rect,
    restore_rect: Ui2Rect,
    z: i16,
    visible: bool,
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
    cursors: Vec<Ui2CursorState>,
    hit_scene: Ui2HitScene,
    last_browser_interactive_seq: u32,
    move_drag: Ui2WindowMoveDrag,
    resize_drag: Ui2WindowResizeDrag,
    scroll_drag: Ui2WindowScrollDrag,
    scroll_pan_drag: Ui2WindowScrollPanDrag,
    windows: Vec<Ui2Window>,
    loadscreen_end_signaled: bool,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi2WindowInfo {
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub decoration_mode: u32,
    pub visible: u32,
    pub selected: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub content_x: i32,
    pub content_y: i32,
    pub content_width: u32,
    pub content_height: u32,
    pub decoration_x: i32,
    pub decoration_y: i32,
    pub decoration_width: u32,
    pub decoration_height: u32,
}

static UI2_STATE: Once<Mutex<Ui2State>> = Once::new();
static UI2_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_DIRTY: AtomicBool = AtomicBool::new(false);
static UI2_BROWSER_WINDOW_ID: AtomicU32 = AtomicU32::new(0);

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
            cursors: Vec::new(),
            hit_scene: Ui2HitScene::default(),
            last_browser_interactive_seq: 0,
            move_drag: Ui2WindowMoveDrag::default(),
            resize_drag: Ui2WindowResizeDrag::default(),
            scroll_drag: Ui2WindowScrollDrag::default(),
            scroll_pan_drag: Ui2WindowScrollPanDrag::default(),
            windows: Vec::new(),
            loadscreen_end_signaled: false,
        };

        let browser_id = alloc_window(
            &mut state,
            Ui2WindowKind::HostedBrowser,
            "Browser 1",
            Ui2Rect::new(
                72.0,
                56.0,
                ((view_w as f32) - 144.0).max(360.0),
                (view_h as f32) - 112.0,
            ),
            10,
            255,
        );
        UI2_BROWSER_WINDOW_ID.store(browser_id, Ordering::Release);
        if let Some(window) = window_mut(&mut state, browser_id) {
            window.horizontal_scrollbar_side = Ui2WindowHorizontalScrollbarSide::Top;
        }
        let _ = trueos_qjs::browser_task::bind_browser_window_to_instance(
            trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID,
            browser_id,
        );

        let browser2_id = alloc_window(
            &mut state,
            Ui2WindowKind::HostedBrowser,
            "Browser 2",
            Ui2Rect::new(
                (view_w as f32 * 0.58).max(420.0),
                92.0,
                ((view_w as f32) * 0.32).max(300.0),
                ((view_h as f32) * 0.56).max(260.0),
            ),
            12,
            255,
        );
        if let Some(window) = window_mut(&mut state, browser2_id) {
            window.browser_instance_id = trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID + 1;
            window.bottom_scrollbar_visible = false;
        }
        let _ = trueos_qjs::browser_task::bind_browser_window_to_instance(
            trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID + 1,
            browser2_id,
        );

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
            trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
        } else {
            0
        },
        title: String::from(title),
        rect,
        restore_rect: rect,
        z,
        visible: true,
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
        trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
    } else {
        window.browser_instance_id
    }
}

fn browser_surface_state_for_window(
    window: &Ui2Window,
) -> trueos_qjs::browser_task::HostedBrowserSurfaceState {
    trueos_qjs::browser_task::hosted_surface_state_for_browser(window_browser_instance_id(window))
}

fn hosted_browser_scroll_max(
    snapshot: &trueos_qjs::browser_task::HostedBrowserSurfaceState,
) -> u32 {
    let viewport_h = snapshot.viewport_height.max(1);
    let content_h = snapshot.content_height.max(viewport_h);
    content_h.saturating_sub(viewport_h)
}

fn clamp_hosted_browser_scroll(
    snapshot: &trueos_qjs::browser_task::HostedBrowserSurfaceState,
    requested_scroll: i64,
) -> u32 {
    let max_scroll = hosted_browser_scroll_max(snapshot) as i64;
    requested_scroll.clamp(0, max_scroll) as u32
}

fn normalized_hosted_browser_scroll(
    snapshot: &trueos_qjs::browser_task::HostedBrowserSurfaceState,
) -> u32 {
    clamp_hosted_browser_scroll(snapshot, snapshot.scroll_y as i64)
}

fn browser_interactive_state_for_window(
    window: &Ui2Window,
) -> trueos_qjs::browser_task::HostedBrowserInteractiveState {
    trueos_qjs::browser_task::hosted_interactive_state_for_browser(window_browser_instance_id(
        window,
    ))
}

fn hosted_browser_interactive_seq(state: &Ui2State) -> u32 {
    state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| {
            let instance_id = window_browser_instance_id(window);
            let seq = trueos_qjs::browser_task::hosted_interactive_seq_for_browser(instance_id);
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
            let seq = trueos_qjs::browser_task::hosted_surface_seq_for_browser(instance_id);
            instance_id.wrapping_mul(1315423911).wrapping_add(seq)
        })
        .fold(0u32, |acc, value| {
            acc.wrapping_mul(16777619).wrapping_add(value)
        })
}

fn window_is_renderable(window: &Ui2Window) -> bool {
    window.visible
}

fn cursor_color(slot_id: u32) -> (u8, u8, u8, u8) {
    match slot_id % 6 {
        0 => (0x3B, 0x82, 0xF6, 0xFF),
        1 => (0xEF, 0x44, 0x44, 0xFF),
        2 => (0x10, 0xB9, 0x81, 0xFF),
        3 => (0xF5, 0x9E, 0x0B, 0xFF),
        4 => (0x8B, 0x5C, 0xF6, 0xFF),
        _ => (0x06, 0xB6, 0xD4, 0xFF),
    }
}

fn cursor_index(state: &Ui2State, slot_id: u32) -> Option<usize> {
    state
        .cursors
        .iter()
        .position(|cursor| cursor.slot_id == slot_id)
}

fn ensure_cursor_index(state: &mut Ui2State, slot_id: u32) -> usize {
    if let Some(idx) = cursor_index(state, slot_id) {
        return idx;
    }
    state.cursors.push(Ui2CursorState {
        slot_id,
        ..Ui2CursorState::default()
    });
    state.cursors.len() - 1
}

fn note_selection_change(window: &mut Ui2Window) {
    window.dirty = true;
    window.last_reason = "cursor-select";
}

fn set_cursor_selected_window(state: &mut Ui2State, slot_id: u32, next_window_id: u32) -> bool {
    let cursor_idx = ensure_cursor_index(state, slot_id);
    if state.cursors[cursor_idx].selected_window_id == next_window_id {
        return false;
    }

    let mut changed = false;
    for window in &mut state.windows {
        if let Some(pos) = window
            .selected_cursor_slots
            .iter()
            .position(|selected_slot_id| *selected_slot_id == slot_id)
        {
            window.selected_cursor_slots.remove(pos);
            note_selection_change(window);
            changed = true;
        }
    }

    if next_window_id != 0 {
        if let Some(window) = window_mut(state, next_window_id) {
            if !window
                .selected_cursor_slots
                .iter()
                .any(|selected_slot_id| *selected_slot_id == slot_id)
            {
                window.selected_cursor_slots.push(slot_id);
                note_selection_change(window);
                changed = true;
            }
        }
    }

    state.cursors[cursor_idx].selected_window_id = next_window_id;
    if changed {
        state.compose_reason = "cursor-select";
        UI2_DIRTY.store(true, Ordering::Release);
        crate::log!(
            "ui2: cursor-select slot={} window={}\n",
            slot_id,
            next_window_id
        );
    }
    changed
}

fn cursor_event_px(value: f64, extent: u32) -> f32 {
    let max_px = extent.saturating_sub(1) as f32;
    (value.clamp(0.0, 1.0) as f32) * max_px
}

struct Ui2HitBuildContext<'a> {
    state: &'a Ui2State,
}

trait Ui2WindowHitSource {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene);
}

impl Ui2WindowHitSource for Ui2Window {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene) {
        if !window_is_renderable(self) {
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
            trueos_qjs::browser_task::hosted_interactive_seq_for_browser(
                window_browser_instance_id(&window),
            );
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

fn process_cursor_event(state: &mut Ui2State, event: crate::usb::hid::TrueosHidCursorEvent) {
    let slot_id = event.slot_id;
    if slot_id == 0 {
        return;
    }

    let px = cursor_event_px(event.x, state.view_w);
    let py = cursor_event_px(event.y, state.view_h);
    let press_hit = state.hit_scene.hit_at(px, py);
    let release_hit = state.hit_scene.hit_at(px, py);
    let press_system_button_action = press_hit.and_then(|target| {
        if target.kind == Ui2HitKind::WindowDecoration {
            system_button_action_at(state, target.owner_window_id, px, py)
        } else {
            None
        }
    });
    let press_window_id = if (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0 {
        press_hit.map(|target| target.owner_window_id).unwrap_or(0)
    } else {
        0
    };
    let release_window_id = release_hit
        .map(|target| target.owner_window_id)
        .unwrap_or(0);
    let cursor_idx = ensure_cursor_index(state, slot_id);

    let mut select_window_id: Option<u32> = None;
    let mut window_button_action: Option<(u32, Ui2SystemButtonAction)> = None;
    let mut begin_move_drag = false;
    let mut begin_resize_drag: Option<(u32, u32)> = None;
    let mut begin_scroll_drag = false;
    let mut begin_scroll_pan_window_id = 0u32;
    let mut click_candidate_window_id = 0u32;
    let mut click_press_x = 0.0f32;
    let mut click_press_y = 0.0f32;
    {
        let cursor = &mut state.cursors[cursor_idx];
        let prev_buttons_down = cursor.buttons_down;
        let primary_was_down = (prev_buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let primary_is_down = (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let middle_was_down = (prev_buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;
        let middle_is_down = (event.buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;

        cursor.x = px;
        cursor.y = py;
        cursor.buttons_down = event.buttons_down;

        if !primary_was_down && primary_is_down {
            cursor.press_x = px;
            cursor.press_y = py;
            cursor.press_window_id = press_window_id;
            cursor.press_armed = press_window_id != 0;
            begin_move_drag = true;
        } else if primary_was_down && !primary_is_down {
            if cursor.press_armed
                && cursor.press_window_id != 0
                && cursor.press_window_id == release_window_id
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                click_candidate_window_id = release_window_id;
                click_press_x = cursor.press_x;
                click_press_y = cursor.press_y;
            } else if cursor.press_window_id == 0
                && release_window_id == 0
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                select_window_id = Some(0);
            }
            cursor.press_armed = false;
            cursor.press_window_id = 0;
        }

        if !middle_was_down
            && middle_is_down
            && let Some(target) = press_hit
            && matches!(
                target.kind,
                Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive
            )
        {
            begin_scroll_pan_window_id = target.owner_window_id;
        }
    }

    if begin_move_drag {
        if let Some(target) = press_hit {
            match target.kind {
                Ui2HitKind::WindowResizeButton => {
                    begin_resize_drag = Some((
                        target.owner_window_id,
                        UI2_WINDOW_RESIZE_RIGHT | UI2_WINDOW_RESIZE_BOTTOM,
                    ));
                }
                Ui2HitKind::WindowVerticalScrollbar => {
                    begin_scroll_drag = true;
                }
                Ui2HitKind::WindowDecoration => {
                    if press_system_button_action.is_none() {
                        let _ = begin_move_drag_for_cursor(
                            state,
                            slot_id,
                            target.owner_window_id,
                            px,
                            py,
                        );
                    }
                }
                Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive => {}
            }
        }
    }
    if let Some((window_id, edge_mask)) = begin_resize_drag {
        let _ = begin_window_resize_for_cursor(state, slot_id, window_id, edge_mask);
    }
    if begin_scroll_drag {
        let _ = begin_vertical_scroll_drag_for_cursor(state, slot_id, px, py);
    }
    if begin_scroll_pan_window_id != 0 {
        let _ =
            begin_window_scroll_pan_for_cursor(state, slot_id, begin_scroll_pan_window_id, px, py);
        let _ = set_cursor_selected_window(state, slot_id, begin_scroll_pan_window_id);
    }

    update_move_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    update_resize_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    let _ = update_scroll_drag_for_cursor(state, slot_id, py, event.buttons_down);
    let _ = update_scroll_pan_for_cursor(state, slot_id, px, py, event.buttons_down);

    if click_candidate_window_id != 0 {
        let press_action = system_button_action_at(
            state,
            click_candidate_window_id,
            click_press_x,
            click_press_y,
        );
        let release_action = system_button_action_at(state, click_candidate_window_id, px, py);
        if let (Some(press_action), Some(release_action)) = (press_action, release_action) {
            if press_action == release_action {
                window_button_action = Some((click_candidate_window_id, release_action));
            } else {
                select_window_id = Some(click_candidate_window_id);
            }
        } else {
            select_window_id = Some(click_candidate_window_id);
        }
    }

    if let Some(select_window_id) = select_window_id {
        let _ = set_cursor_selected_window(state, slot_id, select_window_id);
    }
    if let Some((window_id, action)) = window_button_action {
        let _ = handle_system_button_action(state, window_id, action);
    }

    if event.wheel != 0 {
        let selected_window_id = state.cursors[cursor_idx].selected_window_id;
        let _ = forward_cursor_wheel_to_selected_window(state, selected_window_id, event.wheel);
    }
}

fn forward_cursor_wheel_to_selected_window(
    state: &mut Ui2State,
    window_id: u32,
    wheel: i16,
) -> bool {
    if window_id == 0 || wheel == 0 {
        return false;
    }
    let Some(window) = state.windows.iter().find(|window| window.id == window_id) else {
        return false;
    };
    if !window.visible {
        return false;
    }
    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            let browser_instance_id = window_browser_instance_id(window);
            let snapshot = browser_surface_state_for_window(window);
            let scroll_delta = -(wheel as i32) * UI2_WHEEL_SCROLL_STEP_PX;
            let next_scroll = clamp_hosted_browser_scroll(
                &snapshot,
                i64::from(normalized_hosted_browser_scroll(&snapshot))
                    .saturating_add(i64::from(scroll_delta)),
            );
            if trueos_qjs::browser_task::set_hosted_scroll_y_for_browser(
                browser_instance_id,
                next_scroll,
            ) {
                state.compose_reason = "wheel-scroll";
                true
            } else {
                false
            }
        }
        Ui2WindowKind::HostedSurface => false,
    }
}

fn pump_cursor_selection(state: &mut Ui2State) {
    let mut events = [crate::usb::hid::TrueosHidCursorEvent::default(); UI2_CURSOR_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::usb::hid::read_cursor_events_since(state.cursor_read_seq, &mut events);
        if dropped != 0 {
            crate::log!(
                "ui2: cursor-event-drop read_seq={} dropped={}\n",
                state.cursor_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.cursor_read_seq = next_seq;
        for event in events.iter().take(wrote) {
            process_cursor_event(state, *event);
        }
        if wrote < events.len() {
            break;
        }
    }
}

fn selected_window_id_for_keyboard(state: &Ui2State) -> Option<u32> {
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window.visible
            || window.state == Ui2WindowStateKind::Minimized
            || window.selected_cursor_slots.is_empty()
        {
            continue;
        }
        return Some(window.id);
    }
    None
}

fn keyboard_output_modifiers_to_browser_mask(modifiers: u8) -> u8 {
    let mut out = 0u8;
    if (modifiers & ((1 << 1) | (1 << 5))) != 0 {
        out |= trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_SHIFT;
    }
    if (modifiers & ((1 << 0) | (1 << 4))) != 0 {
        out |= trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_CTRL;
    }
    if (modifiers & ((1 << 2) | (1 << 6))) != 0 {
        out |= trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_ALT;
    }
    if (modifiers & ((1 << 3) | (1 << 7))) != 0 {
        out |= trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_META;
    }
    out
}

fn keyboard_output_key_name(
    event: &crate::v::keyboard::TrueosKeyboardOutputEvent,
) -> Option<String> {
    let named = match event.key_code {
        crate::v::keyboard::KEYBOARD_KEY_BACKSPACE => Some("Backspace"),
        crate::v::keyboard::KEYBOARD_KEY_TAB => Some("Tab"),
        crate::v::keyboard::KEYBOARD_KEY_ENTER => Some("Enter"),
        crate::v::keyboard::KEYBOARD_KEY_ESCAPE => Some("Escape"),
        crate::v::keyboard::KEYBOARD_KEY_SPACE => Some("Space"),
        crate::v::keyboard::KEYBOARD_KEY_DELETE => Some("Delete"),
        crate::v::keyboard::KEYBOARD_KEY_INSERT => Some("Insert"),
        crate::v::keyboard::KEYBOARD_KEY_HOME => Some("Home"),
        crate::v::keyboard::KEYBOARD_KEY_END => Some("End"),
        crate::v::keyboard::KEYBOARD_KEY_PAGE_UP => Some("PageUp"),
        crate::v::keyboard::KEYBOARD_KEY_PAGE_DOWN => Some("PageDown"),
        crate::v::keyboard::KEYBOARD_KEY_ARROW_UP => Some("ArrowUp"),
        crate::v::keyboard::KEYBOARD_KEY_ARROW_DOWN => Some("ArrowDown"),
        crate::v::keyboard::KEYBOARD_KEY_ARROW_LEFT => Some("ArrowLeft"),
        crate::v::keyboard::KEYBOARD_KEY_ARROW_RIGHT => Some("ArrowRight"),
        crate::v::keyboard::KEYBOARD_KEY_F1 => Some("F1"),
        crate::v::keyboard::KEYBOARD_KEY_F2 => Some("F2"),
        crate::v::keyboard::KEYBOARD_KEY_F3 => Some("F3"),
        crate::v::keyboard::KEYBOARD_KEY_F4 => Some("F4"),
        crate::v::keyboard::KEYBOARD_KEY_F5 => Some("F5"),
        crate::v::keyboard::KEYBOARD_KEY_F6 => Some("F6"),
        crate::v::keyboard::KEYBOARD_KEY_F7 => Some("F7"),
        crate::v::keyboard::KEYBOARD_KEY_F8 => Some("F8"),
        crate::v::keyboard::KEYBOARD_KEY_F9 => Some("F9"),
        crate::v::keyboard::KEYBOARD_KEY_F10 => Some("F10"),
        crate::v::keyboard::KEYBOARD_KEY_F11 => Some("F11"),
        crate::v::keyboard::KEYBOARD_KEY_F12 => Some("F12"),
        _ => None,
    };
    if let Some(name) = named {
        return Some(String::from(name));
    }
    char::from_u32(event.codepoint).map(|ch| {
        let mut value = String::new();
        value.push(ch);
        value
    })
}

fn browser_keyboard_event_from_output(
    event: crate::v::keyboard::TrueosKeyboardOutputEvent,
) -> Option<trueos_qjs::browser_task::HostedKeyboardEvent> {
    match event.kind {
        crate::v::keyboard::KEYBOARD_OUTPUT_KIND_TEXT => {
            let utf8_len = (event.utf8_len as usize).min(event.utf8.len());
            if utf8_len == 0 {
                return char::from_u32(event.codepoint).map(|ch| {
                    let mut text = String::new();
                    text.push(ch);
                    trueos_qjs::browser_task::HostedKeyboardEvent::Text { text }
                });
            }
            let text = core::str::from_utf8(&event.utf8[..utf8_len]).ok()?;
            if text.is_empty() {
                return None;
            }
            Some(trueos_qjs::browser_task::HostedKeyboardEvent::Text {
                text: String::from(text),
            })
        }
        crate::v::keyboard::KEYBOARD_OUTPUT_KIND_KEY => {
            let key = keyboard_output_key_name(&event)?;
            Some(trueos_qjs::browser_task::HostedKeyboardEvent::Key {
                key,
                modifiers: keyboard_output_modifiers_to_browser_mask(event.modifiers),
            })
        }
        _ => None,
    }
}

fn pump_keyboard_input(state: &mut Ui2State) {
    let selected_window_id = selected_window_id_for_keyboard(state);
    let mut raw_events =
        [crate::v::keyboard::TrueosKeyboardOutputEvent::default(); UI2_KEYBOARD_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::v::keyboard::read_output_events_since(state.keyboard_read_seq, &mut raw_events);
        if dropped != 0 {
            crate::log!(
                "ui2: keyboard-event-drop read_seq={} dropped={}\n",
                state.keyboard_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.keyboard_read_seq = next_seq;

        if let Some(window_id) = selected_window_id {
            let window_kind = state
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.kind);
            match window_kind {
                Some(Ui2WindowKind::HostedBrowser) => {
                    let mut events = Vec::new();
                    for event in raw_events.iter().take(wrote).copied() {
                        if let Some(next) = browser_keyboard_event_from_output(event) {
                            events.push(next);
                        }
                    }
                    if !events.is_empty()
                        && !trueos_qjs::browser_task::queue_hosted_keyboard_events(
                            window_id,
                            events.as_slice(),
                        )
                    {
                        crate::log!(
                            "ui2: keyboard-forward-drop window={} count={}\n",
                            window_id,
                            events.len()
                        );
                    }
                }
                Some(Ui2WindowKind::HostedSurface) => {
                    for event in raw_events.iter().take(wrote).copied() {
                        let _ = crate::tst_gfx_tetris::queue_ui2_keyboard_event(window_id, event);
                    }
                }
                None => {}
            }
        }

        if wrote < raw_events.len() {
            break;
        }
    }
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
        visible: if window.visible { 1 } else { 0 },
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
    let max_w = (view_w as f32).max(1.0);
    let max_h = (view_h as f32).max(1.0);
    Ui2Rect::new(
        rect.x,
        rect.y,
        rect.w.max(1.0).min(max_w),
        rect.h.max(1.0).min(max_h),
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

fn set_window_rect_in_state(state: &mut Ui2State, id: u32, rect: Ui2Rect, reason: &'static str) -> bool {
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

fn window_edge_drop_action(state: &Ui2State, cursor_x: f32, cursor_y: f32) -> Option<Ui2WindowEdgeDropAction> {
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
        if candidate.0 > UI2_WINDOW_EDGE_DROP_PX {
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

#[inline]
fn window_uses_live_resize(kind: Ui2WindowKind) -> bool {
    !matches!(kind, Ui2WindowKind::HostedBrowser)
}

fn pick_drag_cursor_slot(state: &Ui2State, window: &Ui2Window) -> Option<u32> {
    for slot_id in &window.selected_cursor_slots {
        if let Some(cursor) = state
            .cursors
            .iter()
            .find(|cursor| cursor.slot_id == *slot_id)
        {
            if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0 {
                return Some(*slot_id);
            }
        }
    }
    for cursor in &state.cursors {
        if cursor.selected_window_id == window.id
            && (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0
        {
            return Some(cursor.slot_id);
        }
    }
    for cursor in &state.cursors {
        if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
            continue;
        }
        if rect_contains_point(window.rect, cursor.x, cursor.y) {
            return Some(cursor.slot_id);
        }
    }
    None
}

fn begin_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Maximized {
        return false;
    }
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    if let Some(window_mut) = window_mut(state, window_id) {
        window_mut.z = top_z.saturating_add(1);
        let _ = note_window_dirty(state, window_id, "begin-window-move");
    }
    state.move_drag = Ui2WindowMoveDrag {
        active: true,
        window_id,
        cursor_slot_id: slot_id,
        grab_dx: cursor_x - window.rect.x,
        grab_dy: cursor_y - window.rect.y,
    };
    state.resize_drag = Ui2WindowResizeDrag::default();
    state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    state.compose_reason = "begin-window-move";
    refresh_window_hit_entries(state, window_id);
    true
}

fn begin_window_resize_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    edge_mask: u32,
) -> bool {
    let edge_mask = edge_mask
        & (UI2_WINDOW_RESIZE_LEFT
            | UI2_WINDOW_RESIZE_TOP
            | UI2_WINDOW_RESIZE_RIGHT
            | UI2_WINDOW_RESIZE_BOTTOM);
    if !is_valid_resize_edge_mask(edge_mask) {
        return false;
    }
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        return false;
    }
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }

    state.move_drag = Ui2WindowMoveDrag::default();
    state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    state.resize_drag = Ui2WindowResizeDrag {
        active: true,
        window_id,
        cursor_slot_id: slot_id,
        live_apply: window_uses_live_resize(window.kind),
        edge_mask,
        start_cursor_x: cursor.x,
        start_cursor_y: cursor.y,
        start_rect: window.rect,
        preview_rect: window.rect,
    };
    state.compose_reason = "begin-window-resize";
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    if let Some(window_mut) = window_mut(state, window_id) {
        window_mut.z = top_z.saturating_add(1);
    }
    let noted = note_window_dirty(state, window_id, "begin-window-resize");
    if noted {
        refresh_window_hit_entries(state, window_id);
    }
    noted
}

fn browser_vertical_scrollbar_metrics(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<(Ui2Rect, f32, f32, u32)> {
    if window.kind != Ui2WindowKind::HostedBrowser {
        return None;
    }
    let track = window_vertical_scrollbar_rect(state, window)?;
    let snapshot = browser_surface_state_for_window(window);
    let viewport_h = snapshot.viewport_height.max(1);
    let content_h = snapshot.content_height.max(viewport_h);
    let scroll_range = hosted_browser_scroll_max(&snapshot);
    let thumb_h = libm::fmaxf(
        10.0,
        (track.h * (viewport_h as f32 / content_h as f32)).min(track.h),
    );
    let thumb_y = if scroll_range > 0 {
        let avail = (track.h - thumb_h).max(0.0);
        track.y
            + (avail * (normalized_hosted_browser_scroll(&snapshot) as f32 / scroll_range as f32))
    } else {
        track.y
    };
    Some((track, thumb_h, thumb_y, scroll_range))
}

fn begin_vertical_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window_id) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .map(|cursor| cursor.press_window_id)
        .filter(|id| *id != 0)
    else {
        return false;
    };
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    let Some((track, thumb_h, thumb_y, _)) = browser_vertical_scrollbar_metrics(state, window)
    else {
        return false;
    };
    let thumb_rect = Ui2Rect::new(track.x, thumb_y, track.w, thumb_h);
    let grab_offset = if rect_contains_point(thumb_rect, cursor_x, cursor_y) {
        (cursor_y - thumb_y).clamp(0.0, thumb_h)
    } else {
        thumb_h * 0.5
    };
    state.move_drag = Ui2WindowMoveDrag::default();
    state.resize_drag = Ui2WindowResizeDrag::default();
    state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
    state.scroll_drag = Ui2WindowScrollDrag {
        active: true,
        window_id,
        cursor_slot_id: slot_id,
        track_rect: track,
        thumb_extent: thumb_h,
        grab_offset,
    };
    update_scroll_drag_for_cursor(state, slot_id, cursor_y, UI2_PRIMARY_BUTTON_MASK)
}

fn begin_window_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    if window.kind != Ui2WindowKind::HostedBrowser {
        return false;
    }
    state.move_drag = Ui2WindowMoveDrag::default();
    state.resize_drag = Ui2WindowResizeDrag::default();
    state.scroll_drag = Ui2WindowScrollDrag::default();
    state.scroll_pan_drag = Ui2WindowScrollPanDrag {
        active: true,
        window_id,
        cursor_slot_id: slot_id,
        last_cursor_x: cursor_x,
        last_cursor_y: cursor_y,
    };
    state.compose_reason = "begin-scroll-pan";
    true
}

fn update_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    if !state.scroll_drag.active || state.scroll_drag.cursor_slot_id != slot_id {
        return false;
    }
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        state.scroll_drag = Ui2WindowScrollDrag::default();
        return false;
    }
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == state.scroll_drag.window_id && window_is_renderable(window))
    else {
        state.scroll_drag = Ui2WindowScrollDrag::default();
        return false;
    };
    let Some((track, _thumb_h, _thumb_y, scroll_range)) =
        browser_vertical_scrollbar_metrics(state, window)
    else {
        state.scroll_drag = Ui2WindowScrollDrag::default();
        return false;
    };
    state.scroll_drag.track_rect = track;
    if scroll_range == 0 {
        return false;
    }
    let avail = (track.h - state.scroll_drag.thumb_extent).max(0.0);
    if avail <= 0.0 {
        return false;
    }
    let thumb_y = (cursor_y - state.scroll_drag.grab_offset).clamp(track.y, track.y + avail);
    let ratio = ((thumb_y - track.y) / avail).clamp(0.0, 1.0);
    let next_scroll = clamp_hosted_browser_scroll(
        &browser_surface_state_for_window(window),
        libm::roundf(ratio * scroll_range as f32) as i64,
    );
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == state.scroll_drag.window_id)
    else {
        return false;
    };
    if trueos_qjs::browser_task::set_hosted_scroll_y_for_browser(
        window_browser_instance_id(window),
        next_scroll,
    ) {
        state.compose_reason = "scrollbar-drag";
        true
    } else {
        false
    }
}

fn update_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    if !state.scroll_pan_drag.active || state.scroll_pan_drag.cursor_slot_id != slot_id {
        return false;
    }
    if (buttons_down & UI2_MIDDLE_BUTTON_MASK) == 0 {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
        return false;
    }
    let drag = state.scroll_pan_drag;
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id && window_is_renderable(window))
    else {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
        return false;
    };
    if window.kind != Ui2WindowKind::HostedBrowser {
        state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
        return false;
    }

    let dx = cursor_x - drag.last_cursor_x;
    let dy = cursor_y - drag.last_cursor_y;
    state.scroll_pan_drag.last_cursor_x = cursor_x;
    state.scroll_pan_drag.last_cursor_y = cursor_y;

    let dy_px = libm::roundf(dy) as i32;
    if dy_px == 0 {
        return false;
    }

    let snapshot = browser_surface_state_for_window(window);
    let next_scroll = clamp_hosted_browser_scroll(
        &snapshot,
        i64::from(normalized_hosted_browser_scroll(&snapshot)).saturating_sub(i64::from(dy_px)),
    );
    if trueos_qjs::browser_task::set_hosted_scroll_y_for_browser(
        window_browser_instance_id(window),
        next_scroll,
    ) {
        let _ = dx;
        state.compose_reason = "scroll-pan";
        true
    } else {
        false
    }
}

fn update_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    if !state.move_drag.active || state.move_drag.cursor_slot_id != slot_id {
        return;
    }
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        let window_id = state.move_drag.window_id;
        state.move_drag = Ui2WindowMoveDrag::default();
        if let Some(action) = window_edge_drop_action(state, cursor_x, cursor_y) {
            let _ = apply_window_edge_drop_action(state, window_id, action);
        }
        return;
    }
    let next_x = cursor_x - state.move_drag.grab_dx;
    let next_y = cursor_y - state.move_drag.grab_dy;
    let window_id = state.move_drag.window_id;
    let Some(window) = window_mut(state, window_id) else {
        state.move_drag = Ui2WindowMoveDrag::default();
        return;
    };
    if window.state == Ui2WindowStateKind::Maximized {
        return;
    }
    let mut moved = false;
    if window.rect.x != next_x || window.rect.y != next_y {
        window.rect.x = next_x;
        window.rect.y = next_y;
        window.restore_rect = window.rect;
        moved = true;
    }
    if moved {
        state.compose_reason = "window-drag";
        let _ = note_window_dirty(state, window_id, "window-drag");
        let _ = note_window_viewport_sync_needed(state, window_id);
        refresh_window_hit_entries(state, window_id);
    }
}

fn update_resize_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    if !state.resize_drag.active || state.resize_drag.cursor_slot_id != slot_id {
        return;
    }
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        let drag = state.resize_drag;
        if drag.active && !drag.live_apply && drag.preview_rect != drag.start_rect {
            if let Some(window) = window_mut(state, drag.window_id) {
                window.rect = drag.preview_rect;
                window.restore_rect = drag.preview_rect;
            }
            state.compose_reason = "window-resize-commit";
            let _ = commit_window_geometry_change(state, drag.window_id, "window-resize-commit");
        } else if drag.active && !drag.live_apply {
            state.compose_reason = "window-resize-cancel";
            let _ = note_window_dirty(state, drag.window_id, "window-resize-cancel");
        }
        state.resize_drag = Ui2WindowResizeDrag::default();
        return;
    }

    let drag = state.resize_drag;
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id)
    else {
        state.resize_drag = Ui2WindowResizeDrag::default();
        return;
    };
    if window.state == Ui2WindowStateKind::Maximized {
        state.resize_drag = Ui2WindowResizeDrag::default();
        return;
    }

    let mut next = drag.start_rect;
    let dx = cursor_x - drag.start_cursor_x;
    let dy = cursor_y - drag.start_cursor_y;
    let right = drag.start_rect.x + drag.start_rect.w;
    let bottom = drag.start_rect.y + drag.start_rect.h;

    if (drag.edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 {
        let max_x = right - 1.0;
        next.x = libm::fminf(drag.start_rect.x + dx, max_x);
        next.w = (right - next.x).max(1.0);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
        next.w = (drag.start_rect.w + dx).max(1.0);
    }

    if (drag.edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 {
        let max_y = bottom - 1.0;
        next.y = libm::fminf(drag.start_rect.y + dy, max_y);
        next.h = (bottom - next.y).max(1.0);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
        next.h = (drag.start_rect.h + dy).max(1.0);
    }

    if drag.live_apply {
        if window.rect != next {
            if let Some(window) = window_mut(state, drag.window_id) {
                window.rect = next;
                window.restore_rect = next;
            }
            state.compose_reason = "window-resize-drag";
            let _ = note_window_dirty(state, drag.window_id, "window-resize-drag");
            let _ = note_window_viewport_sync_needed(state, drag.window_id);
            refresh_window_hit_entries(state, drag.window_id);
        }
    } else if drag.preview_rect != next {
        state.resize_drag.preview_rect = next;
        state.compose_reason = "window-resize-preview";
        let _ = note_window_dirty(state, drag.window_id, "window-resize-preview");
    }
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
    browser_instance_id: u32,
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
            queue_browser_window_viewport(browser_instance_id, content);
            true
        }
        Ui2WindowKind::HostedSurface => true,
    }
}

fn sync_pending_window_containers(state: &mut Ui2State) {
    let pending: Vec<(u32, bool, Ui2WindowKind, u32, Option<Ui2Rect>)> = state
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
    for (id, renderable, kind, browser_instance_id, content) in pending {
        if sync_window_container(renderable, kind, browser_instance_id, content) {
            synced_ids.push(id);
        }
    }
    for id in synced_ids {
        if let Some(window) = window_mut(state, id) {
            window.container_sync_needed = false;
        }
    }
}

pub fn browser_window_id() -> Option<u32> {
    let id = UI2_BROWSER_WINDOW_ID.load(Ordering::Acquire);
    if id == 0 { None } else { Some(id) }
}

pub fn browser_window_id_for_instance(browser_instance_id: u32) -> Option<u32> {
    let browser_instance_id = if browser_instance_id == 0 {
        trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
    } else {
        browser_instance_id
    };
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| {
            window.kind == Ui2WindowKind::HostedBrowser
                && window_browser_instance_id(window) == browser_instance_id
        })
        .map(|window| window.id)
}

pub fn window_info_by_id(id: u32) -> Option<TrueosUi2WindowInfo> {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .map(|window| window_info(&state, window))
}

pub fn is_window_minimized(id: u32) -> bool {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .map(|window| window.state == Ui2WindowStateKind::Minimized)
        .unwrap_or(false)
}

pub fn create_window(title: &str, rect: Ui2Rect, z: i16, alpha: u8) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedSurface,
        title,
        rect,
        z,
        alpha,
    );
    state.compose_reason = "create-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_browser_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    browser_instance_id: u32,
) -> u32 {
    let browser_instance_id = if browser_instance_id == 0 {
        trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
    } else {
        browser_instance_id
    };
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedBrowser,
        title,
        rect,
        z,
        alpha,
    );
    if let Some(window) = window_mut(&mut state, id) {
        window.browser_instance_id = browser_instance_id;
    }
    if browser_instance_id == trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID {
        UI2_BROWSER_WINDOW_ID.store(id, Ordering::Release);
    }
    let _ = trueos_qjs::browser_task::bind_browser_window_to_instance(browser_instance_id, id);
    state.compose_reason = "create-browser-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_surface_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedSurface,
        title,
        rect,
        z,
        alpha,
    );
    if let Some(window) = window_mut(&mut state, id) {
        window.content_tex_id = tex_id;
        window.content_tex_blend = blend_enabled;
    }
    state.compose_reason = "create-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_surface_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let rect = window_rect_for_content(Ui2WindowDecorationMode::System, content_rect);
    create_hosted_surface_window(title, rect, z, alpha, tex_id, blend_enabled)
}

pub fn set_window_hosted_surface_content(id: u32, tex_id: u32, blend_enabled: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.content_tex_id == tex_id && window.content_tex_blend == blend_enabled {
        return true;
    }
    window.content_tex_id = tex_id;
    window.content_tex_blend = blend_enabled;
    state.compose_reason = "texture-window";
    let noted = note_window_dirty(&mut state, id, "texture-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
    }
    noted
}

#[allow(dead_code)]
pub fn create_texture_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    create_hosted_surface_window(title, rect, z, alpha, tex_id, blend_enabled)
}

#[allow(dead_code)]
pub fn create_texture_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    create_hosted_surface_content_window(title, content_rect, z, alpha, tex_id, blend_enabled)
}

#[allow(dead_code)]
pub fn set_window_texture_content(id: u32, tex_id: u32, blend_enabled: bool) -> bool {
    set_window_hosted_surface_content(id, tex_id, blend_enabled)
}

pub struct Ui2SurfaceWindow {
    window_id: u32,
    tex_id: u32,
    width: u32,
    height: u32,
}

impl Ui2SurfaceWindow {
    pub fn new(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
        clear_rgba: [u8; 4],
    ) -> Option<Self> {
        let width = (content_rect.w.max(1.0) + 0.5) as u32;
        let height = (content_rect.h.max(1.0) + 0.5) as u32;
        let pixels = alloc::vec![clear_rgba; (width as usize) * (height as usize)]
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
                tex_id,
                width,
                height,
                pixels.as_ptr(),
                pixels.len(),
            )
        };
        if rc != 0 {
            crate::log!(
                "ui2-surface-window: init upload failed tex={} rc={} size={}x{}\n",
                tex_id,
                rc,
                width,
                height
            );
            return None;
        }

        let window_id = create_hosted_surface_content_window(
            title,
            Ui2Rect {
                x: content_rect.x,
                y: content_rect.y,
                w: width as f32,
                h: height as f32,
            },
            z,
            alpha,
            tex_id,
            blend_enabled,
        );
        Some(Self {
            window_id,
            tex_id,
            width,
            height,
        })
    }

    #[inline]
    pub fn window_id(&self) -> u32 {
        self.window_id
    }

    #[inline]
    pub fn tex_id(&self) -> u32 {
        self.tex_id
    }

    #[inline]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn render_rgb_triangles(
        &self,
        clear_rgb: u32,
        verts: &[u8],
        repaint_reason: &'static str,
    ) -> bool {
        let rc = crate::surface::io::cabi::render_rgb_triangles_to_texture(
            self.tex_id,
            clear_rgb,
            verts,
        );
        if rc != 0 {
            crate::log!(
                "ui2-surface-window: rgb render failed window={} tex={} rc={}\n",
                self.window_id,
                self.tex_id,
                rc
            );
            return false;
        }
        if is_window_minimized(self.window_id) {
            return true;
        }
        request_window_content_present(self.window_id, repaint_reason)
    }

    #[allow(dead_code)]
    pub fn upload_rgba(&self, pixels: &[u8], repaint_reason: &'static str) -> bool {
        let expected = self.width as usize * self.height as usize * 4;
        if pixels.len() != expected {
            crate::log!(
                "ui2-surface-window: upload size mismatch tex={} got={} expected={}\n",
                self.tex_id,
                pixels.len(),
                expected
            );
            return false;
        }
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
                self.tex_id,
                self.width,
                self.height,
                pixels.as_ptr(),
                pixels.len(),
            )
        };
        if rc != 0 {
            crate::log!(
                "ui2-surface-window: rgba upload failed window={} tex={} rc={}\n",
                self.window_id,
                self.tex_id,
                rc
            );
            return false;
        }
        if is_window_minimized(self.window_id) {
            return true;
        }
        request_window_content_present(self.window_id, repaint_reason)
    }
}

pub fn window_content_rect_by_id(id: u32) -> Option<Ui2Rect> {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window_content_rect(&state, window))
}

pub fn move_window(id: u32, x: f32, y: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        window.state = Ui2WindowStateKind::Normal;
    }
    window.rect.x = x;
    window.rect.y = y;
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = window.rect;
    }
    state.compose_reason = "move-window";
    let noted = note_window_dirty(&mut state, id, "move-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn resize_window(id: u32, w: f32, h: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        window.state = Ui2WindowStateKind::Normal;
    }
    window.rect.w = w.max(1.0);
    window.rect.h = h.max(1.0);
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = window.rect;
    }
    state.compose_reason = "resize-window";
    let noted = note_window_dirty(&mut state, id, "resize-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn set_window_title(id: u32, title: &str) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.title == title {
        return true;
    }
    window.title = String::from(title);
    state.compose_reason = "title-window";
    note_window_dirty(&mut state, id, "title-window")
}

pub fn raise_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    window.z = top_z.saturating_add(1);
    state.compose_reason = "raise-window";
    let noted = note_window_dirty(&mut state, id, "raise-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn focus_window(id: u32) -> bool {
    raise_window(id)
}

pub fn set_window_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    set_window_visible_in_state(&mut state, id, visible)
}

pub fn close_window(id: u32) -> bool {
    set_window_visible(id, false)
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
        if !visible && state.move_drag.window_id == id {
            state.move_drag = Ui2WindowMoveDrag::default();
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
        if !visible && state.resize_drag.window_id == id {
            state.resize_drag = Ui2WindowResizeDrag::default();
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
        if !visible && state.scroll_drag.window_id == id {
            state.scroll_drag = Ui2WindowScrollDrag::default();
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
        if !visible && state.scroll_drag.window_id == id {
            state.scroll_drag = Ui2WindowScrollDrag::default();
        }
    }
    noted
}

pub fn set_window_vertical_scrollbar_side(
    id: u32,
    side: Ui2WindowVerticalScrollbarSide,
) -> bool {
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
        if state.scroll_drag.window_id == id {
            state.scroll_drag = Ui2WindowScrollDrag::default();
        }
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

pub fn minimize_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    minimize_window_in_state(&mut state, id)
}

pub fn maximize_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    maximize_window_in_state(&mut state, id)
}

pub fn restore_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    restore_window_in_state(&mut state, id)
}

pub fn begin_window_move(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    let Some(cursor_slot_id) = pick_drag_cursor_slot(&state, &window) else {
        return false;
    };
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == cursor_slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }
    state.move_drag = Ui2WindowMoveDrag {
        active: true,
        window_id: id,
        cursor_slot_id,
        grab_dx: cursor.x - window.rect.x,
        grab_dy: cursor.y - window.rect.y,
    };
    state.resize_drag = Ui2WindowResizeDrag::default();
    state.compose_reason = "begin-window-move";
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    if let Some(window_mut) = window_mut(&mut state, id) {
        window_mut.z = top_z.saturating_add(1);
    }
    let noted = note_window_dirty(&mut state, id, "begin-window-move");
    if noted {
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn begin_window_resize(id: u32, edge_mask: u32) -> bool {
    let edge_mask = edge_mask
        & (UI2_WINDOW_RESIZE_LEFT
            | UI2_WINDOW_RESIZE_TOP
            | UI2_WINDOW_RESIZE_RIGHT
            | UI2_WINDOW_RESIZE_BOTTOM);
    if !is_valid_resize_edge_mask(edge_mask) {
        return false;
    }

    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        return false;
    }
    let Some(cursor_slot_id) = pick_drag_cursor_slot(&state, &window) else {
        return false;
    };
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == cursor_slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }

    begin_window_resize_for_cursor(&mut state, cursor_slot_id, id, edge_mask)
}

fn request_window_composite(id: u32, reason: &'static str) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    state.compose_reason = reason;
    note_window_dirty(&mut state, id, reason)
}

pub fn request_window_content_present(id: u32, reason: &'static str) -> bool {
    request_window_composite(id, reason)
}

pub fn request_window_repaint(id: u32, reason: &'static str) -> bool {
    request_window_content_present(id, reason)
}

pub fn request_browser_repaint(reason: &'static str) -> bool {
    let Some(id) = browser_window_id() else {
        return false;
    };
    request_window_content_present(id, reason)
}

fn is_primary_browser_window(window: &Ui2Window) -> bool {
    matches!(window.kind, Ui2WindowKind::HostedBrowser)
}

pub fn request_full_recompose(reason: &'static str) {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    state.compose_reason = reason;
    for window in &mut state.windows {
        window.dirty = true;
        window.last_reason = reason;
    }
    UI2_DIRTY.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui2_primary_browser_window_id() -> u32 {
    browser_window_id().unwrap_or(0)
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

fn window_decoration_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
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

fn window_bottom_bar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
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
    Some(Ui2Rect::new(
        rect.x,
        rect.y + rect.h - bar_h,
        rect.w,
        bar_h,
    ))
}

fn window_bottom_scrollbar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    if window.decoration_mode != Ui2WindowDecorationMode::System || !window.bottom_scrollbar_visible {
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
    Some(Ui2Rect::new(
        rect.x,
        y,
        rect.w.max(1.0),
        scrollbar_h,
    ))
}

fn window_content_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
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

fn window_vertical_scrollbar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
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
    Some(Ui2Rect::new(
        x,
        rect.y + top_inset,
        w,
        h,
    ))
}

fn window_horizontal_scrollbar_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
    window_bottom_scrollbar_rect(state, window)
}

fn window_bottom_resize_button_rect(state: &Ui2State, window: &Ui2Window) -> Option<Ui2Rect> {
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

fn window_system_button_rect(
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
    let x = match action {
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

fn system_button_action_at(
    state: &Ui2State,
    window_id: u32,
    x: f32,
    y: f32,
) -> Option<Ui2SystemButtonAction> {
    let window = state.windows.iter().find(|window| window.id == window_id)?;
    for action in [
        Ui2SystemButtonAction::Minimize,
        Ui2SystemButtonAction::ToggleMaximize,
        Ui2SystemButtonAction::Close,
    ] {
        let rect = window_system_button_rect(state, window, action)?;
        if rect_contains_point(rect, x, y) {
            return Some(action);
        }
    }
    None
}

fn window_rect_for_content(mode: Ui2WindowDecorationMode, content_rect: Ui2Rect) -> Ui2Rect {
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
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: bottom,
            u: u1,
            v: v1,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: left,
            y: top,
            u: u0,
            v: v0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    ];
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_set_blend(
            if blend_enabled { 1 } else { 0 },
            0x0302,
            0x0303,
            0x0302,
            0x0303,
            0,
            0,
        )
    };
    let rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr() as *const u8,
            verts.len() * core::mem::size_of::<Ui2TexVertex>(),
        )
    };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
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
    )
}

fn queue_browser_window_viewport(browser_instance_id: u32, content: Ui2Rect) {
    let viewport_w = round_to_u32(content.w, 1);
    let viewport_h = round_to_u32(content.h, 1);
    let content_x = libm::roundf(content.x) as i32;
    let content_y = libm::roundf(content.y) as i32;
    let _ = trueos_qjs::browser_task::set_hosted_viewport_for_browser(
        browser_instance_id,
        viewport_w,
        viewport_h,
        content_x,
        content_y,
        viewport_w,
        viewport_h,
    );
}

fn draw_browser_window_content(state: &Ui2State, window: &Ui2Window, content: Ui2Rect) -> bool {
    let snapshot = browser_surface_state_for_window(window);
    if snapshot.regions.is_empty() || snapshot.viewport_width == 0 || snapshot.viewport_height == 0
    {
        return false;
    }

    let sx = round_to_u32(content.x.max(0.0), 0);
    let sy = round_to_u32(content.y.max(0.0), 0);
    let sw = round_to_u32(content.w, 1);
    let sh = round_to_u32(content.h, 1);
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_scissor(sx, sy, sw, sh) };

    let draw_w = snapshot.viewport_width.max(1);
    let draw_h = snapshot.viewport_height.max(1);
    let scroll_top = snapshot
        .content_top_y
        .saturating_add(normalized_hosted_browser_scroll(&snapshot));
    let scroll_bottom = scroll_top.saturating_add(draw_h);
    let mut drew = false;

    for region in &snapshot.regions {
        let tex_id = region.tex_id;
        if tex_id == 0 || region.width == 0 || region.height == 0 {
            continue;
        }
        let doc_y = region.doc_y;
        let doc_bottom = doc_y.saturating_add(region.height);
        if doc_bottom <= scroll_top || doc_y >= scroll_bottom {
            continue;
        }

        let src_top = core::cmp::max(doc_y, scroll_top);
        let src_bottom = core::cmp::min(doc_bottom, scroll_bottom);
        let src_height = src_bottom.saturating_sub(src_top);
        if src_height == 0 {
            continue;
        }

        let src_offset_y = src_top.saturating_sub(doc_y);
        let dest_y = src_top.saturating_sub(scroll_top);
        let draw_width = core::cmp::min(draw_w, region.width).max(1);
        let u0 = 0.0;
        let u1 = (draw_width as f32) / (region.width.max(1) as f32);
        let v0 = (src_offset_y as f32) / (region.height.max(1) as f32);
        let v1 = ((src_offset_y + src_height) as f32) / (region.height.max(1) as f32);

        drew |= draw_texture_rect_uv_no_present(
            tex_id,
            content.x,
            content.y + dest_y as f32,
            draw_width as f32,
            src_height as f32,
            u0,
            v0,
            u1,
            v1,
            state.view_w,
            state.view_h,
            true,
        );
    }

    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_scissor() };
    drew
}

fn log_browser_surface_updates(state: &mut Ui2State) {
    let browser_window_ids: Vec<u32> = state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| window.id)
        .collect();

    for window_id in browser_window_ids {
        let Some(window_snapshot) = state
            .windows
            .iter()
            .find(|window| window.id == window_id)
            .cloned()
        else {
            continue;
        };
        let snapshot = browser_surface_state_for_window(&window_snapshot);
        let Some(window) = window_mut(state, window_id) else {
            continue;
        };
        if window.last_logged_browser_surface_seq == snapshot.seq {
            continue;
        }
        window.last_logged_browser_surface_seq = snapshot.seq;
        let browser_instance_id = window_browser_instance_id(window);
        crate::log!(
            "ui2: browser-snapshot browser_instance={} window_id={} seq={} viewport={}x{} content_h={} content_top_y={} scroll_y={} regions={}\n",
            browser_instance_id,
            window.id,
            snapshot.seq,
            snapshot.viewport_width,
            snapshot.viewport_height,
            snapshot.content_height,
            snapshot.content_top_y,
            snapshot.scroll_y,
            snapshot.regions.len()
        );
        for (idx, region) in snapshot.regions.iter().take(4).enumerate() {
            crate::log!(
                "ui2: browser-region browser_instance={} window_id={} idx={} tex={} doc_y={} size={}x{} rev={} dirty={}\n",
                browser_instance_id,
                window.id,
                idx,
                region.tex_id,
                region.doc_y,
                region.width,
                region.height,
                region.revision,
                if region.dirty { 1 } else { 0 }
            );
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
    let _ = unsafe {
        crate::gfx::lyon::trueos_cabi_gfx_draw_lyon_icon_no_present(
            icon_id,
            0,
            1,
            icon_x,
            icon_y,
            state.view_w,
            state.view_h,
        )
    };
}

fn draw_window_bottom_resize_button(state: &Ui2State, window: &Ui2Window) {
    let Some(rect) = window_bottom_resize_button_rect(state, window) else {
        return;
    };
    let icon_side = 16.0f32;
    let icon_x = rect.x + ((rect.w - icon_side) * 0.5);
    let icon_y = rect.y + ((rect.h - icon_side) * 0.5);
    let _ = unsafe {
        crate::gfx::lyon::trueos_cabi_gfx_draw_lyon_icon_no_present(
            1,
            0,
            1,
            icon_x,
            icon_y,
            state.view_w,
            state.view_h,
        )
    };
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
        let thumb_w = libm::fminf((hbar.w - 2.0).max(8.0), 18.0);
        let thumb_x = hbar.x + ((hbar.w - thumb_w) * 0.5);
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
    let title_rgba = (0xF3, 0xF4, 0xF6, 0xFF);
    let body_rgba = (0xFB, 0xFB, 0xF8, window.alpha);
    let selection_rgba = window
        .selected_cursor_slots
        .first()
        .map(|slot_id| cursor_color(*slot_id))
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
        let mut marker_x = rect.x + rect.w - 14.0;
        let marker_y = rect.y + 8.0;
        for slot_id in window.selected_cursor_slots.iter().take(6) {
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                marker_x,
                marker_y,
                6.0,
                10.0,
                cursor_color(*slot_id),
                state.view_w,
                state.view_h,
            );
            marker_x -= 9.0;
        }
    }

    if window.decoration_mode == Ui2WindowDecorationMode::System && window.titlebar_visible {
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            window.title.as_bytes(),
            rect.x + 10.0,
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
            }
        }
        Ui2WindowKind::HostedSurface => {
            if let Some(content) = content_rect
                && draw_texture_rect_no_present(
                    window.content_tex_id,
                    content.x,
                    content.y,
                    content.w,
                    content.h,
                    state.view_w,
                    state.view_h,
                    window.content_tex_blend,
                )
            {
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
                if UI2_ENABLE_VERBOSE_COMPOSE_LOGS {
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
        if UI2_ENABLE_VERBOSE_COMPOSE_LOGS {
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
        unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
        for idx in sorted_window_indices(state) {
            let window = &state.windows[idx];
            draw_window_frame(state, window);
        }
        draw_resize_preview_outline(state);
        unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
        if state.compose_seq <= 2 {
            crate::log!("ui2: compose-frame seq={} end\n", state.compose_seq);
        }
    });

    if !state.loadscreen_end_signaled {
        crate::v::readiness::set(crate::v::readiness::LOADSCREEN_END);
        state.loadscreen_end_signaled = true;
    }
}

#[embassy_executor::task]
pub async fn ui2_task() {
    if UI2_STARTED.swap(true, Ordering::SeqCst) {
        crate::log!("ui2: already running\n");
        return;
    }

    crate::gfx::init(crate::limine::framebuffer_response());
    init_state();
    request_full_recompose("boot");
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
            pump_cursor_selection(&mut state);
            pump_keyboard_input(&mut state);
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
            compose_windows(&mut state);
            if loop_seq <= 4 {
                crate::log!("ui2: compose seq={} done\n", loop_seq);
            }
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
