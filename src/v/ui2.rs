use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowKind {
    Browser,
    Dialog,
    Menu,
    Overlay,
}

impl Ui2WindowKind {
    #[inline]
    const fn name(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Dialog => "dialog",
            Self::Menu => "menu",
            Self::Overlay => "overlay",
        }
    }
}

#[derive(Copy, Clone, Debug)]
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

#[derive(Clone)]
struct Ui2Window {
    id: u32,
    kind: Ui2WindowKind,
    title: String,
    rect: Ui2Rect,
    z: i16,
    visible: bool,
    alpha: u8,
    dirty: bool,
    dirty_seq: u32,
    last_reason: &'static str,
}

struct Ui2State {
    view_w: u32,
    view_h: u32,
    next_window_id: u32,
    compose_seq: u32,
    compose_reason: &'static str,
    windows: Vec<Ui2Window>,
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
            windows: Vec::new(),
        };

        let browser_id = alloc_window(
            &mut state,
            Ui2WindowKind::Browser,
            "Browser",
            Ui2Rect::new(72.0, 56.0, (view_w as f32) - 144.0, (view_h as f32) - 112.0),
            10,
            255,
        );
        UI2_BROWSER_WINDOW_ID.store(browser_id, Ordering::Release);

        let _ = alloc_window(
            &mut state,
            Ui2WindowKind::Dialog,
            "Dialog",
            Ui2Rect::new(140.0, 108.0, 360.0, 220.0),
            20,
            246,
        );

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
        title: String::from(title),
        rect,
        z,
        visible: true,
        alpha,
        dirty: true,
        dirty_seq: 0,
        last_reason: "create",
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

fn note_window_dirty(state: &mut Ui2State, id: u32, reason: &'static str) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.dirty = true;
    window.last_reason = reason;
    UI2_DIRTY.store(true, Ordering::Release);
    true
}

pub fn browser_window_id() -> Option<u32> {
    let id = UI2_BROWSER_WINDOW_ID.load(Ordering::Acquire);
    if id == 0 { None } else { Some(id) }
}

pub fn create_window(kind: Ui2WindowKind, title: &str, rect: Ui2Rect, z: i16, alpha: u8) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(&mut state, kind, title, rect, z, alpha);
    state.compose_reason = "create-window";
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn move_window(id: u32, x: f32, y: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    window.rect.x = x;
    window.rect.y = y;
    state.compose_reason = "move-window";
    note_window_dirty(&mut state, id, "move-window")
}

pub fn resize_window(id: u32, w: f32, h: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    window.rect.w = w.max(1.0);
    window.rect.h = h.max(1.0);
    state.compose_reason = "resize-window";
    note_window_dirty(&mut state, id, "resize-window")
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
    note_window_dirty(&mut state, id, "raise-window")
}

pub fn set_window_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    window.visible = visible;
    let reason = if visible {
        "show-window"
    } else {
        "hide-window"
    };
    state.compose_reason = reason;
    note_window_dirty(&mut state, id, reason)
}

pub fn request_window_repaint(id: u32, reason: &'static str) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    state.compose_reason = reason;
    note_window_dirty(&mut state, id, reason)
}

pub fn request_browser_repaint(reason: &'static str) -> bool {
    let Some(id) = browser_window_id() else {
        return false;
    };
    request_window_repaint(id, reason)
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

fn draw_window_frame(state: &Ui2State, window: &Ui2Window) {
    if !window.visible {
        return;
    }

    let frame_rgba = match window.kind {
        Ui2WindowKind::Browser => (0x18, 0x1B, 0x20, 0xFF),
        Ui2WindowKind::Dialog => (0x2B, 0x2B, 0x2B, 0xFF),
        Ui2WindowKind::Menu => (0x1F, 0x1A, 0x12, 0xFF),
        Ui2WindowKind::Overlay => (0x08, 0x08, 0x08, 0xC0),
    };
    let title_rgba = match window.kind {
        Ui2WindowKind::Browser => (0xF3, 0xF4, 0xF6, 0xFF),
        Ui2WindowKind::Dialog => (0xF8, 0xF1, 0xE7, 0xFF),
        Ui2WindowKind::Menu => (0xF8, 0xE8, 0xC8, 0xFF),
        Ui2WindowKind::Overlay => (0xFF, 0xFF, 0xFF, 0xFF),
    };
    let body_rgba = match window.kind {
        Ui2WindowKind::Browser => (0xFB, 0xFB, 0xF8, window.alpha),
        Ui2WindowKind::Dialog => (0xF2, 0xEA, 0xDE, window.alpha),
        Ui2WindowKind::Menu => (0xF1, 0xE4, 0xC7, window.alpha),
        Ui2WindowKind::Overlay => (0x12, 0x12, 0x14, window.alpha),
    };
    let shadow_rgba = (0x08, 0x0A, 0x0F, 0x60);
    let border_rgba = (0x08, 0x0A, 0x0F, 0xFF);
    let title_h = 26.0f32;

    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x + 8.0,
        window.rect.y + 10.0,
        window.rect.w,
        window.rect.h,
        shadow_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x,
        window.rect.y,
        window.rect.w,
        window.rect.h,
        body_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x,
        window.rect.y,
        window.rect.w,
        title_h,
        frame_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x,
        window.rect.y,
        window.rect.w,
        1.0,
        border_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x,
        window.rect.y + window.rect.h - 1.0,
        window.rect.w,
        1.0,
        border_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x,
        window.rect.y,
        1.0,
        window.rect.h,
        border_rgba,
        state.view_w,
        state.view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        window.rect.x + window.rect.w - 1.0,
        window.rect.y,
        1.0,
        window.rect.h,
        border_rgba,
        state.view_w,
        state.view_h,
    );

    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        window.title.as_bytes(),
        window.rect.x + 10.0,
        window.rect.y + 5.0,
        state.view_w,
        state.view_h,
        title_rgba.3,
    );

    let body_msg: &[u8] = match window.kind {
        Ui2WindowKind::Browser => &b"browser surface; content compositor slot"[..],
        Ui2WindowKind::Dialog => &b"dialog surface; partial invalidation target"[..],
        Ui2WindowKind::Menu => &b"menu surface; independent top-level alpha"[..],
        Ui2WindowKind::Overlay => &b"overlay surface; translucent top-level pass"[..],
    };
    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        body_msg,
        window.rect.x + 12.0,
        window.rect.y + title_h + 10.0,
        state.view_w,
        state.view_h,
        220,
    );
}

fn compose_windows(state: &mut Ui2State) {
    let dirty_count = state.windows.iter().filter(|window| window.dirty).count();
    for window in &mut state.windows {
        if window.dirty {
            window.dirty_seq = window.dirty_seq.wrapping_add(1);
            crate::log!(
                "ui2: window-update id={} kind={} seq={} reason={}\n",
                window.id,
                window.kind.name(),
                window.dirty_seq,
                window.last_reason
            );
            window.dirty = false;
        }
    }

    state.compose_seq = state.compose_seq.wrapping_add(1);
    crate::log!(
        "ui2: compose seq={} windows={} dirty={} reason={}\n",
        state.compose_seq,
        state.windows.len(),
        dirty_count,
        state.compose_reason
    );

    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        draw_window_frame(state, window);
    }
    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
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

    loop {
        if UI2_DIRTY.swap(false, Ordering::AcqRel) {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            compose_windows(&mut state);
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
