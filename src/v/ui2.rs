use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

const UI2_TITLE_H: f32 = 26.0;
const UI2_NOTES_SURFACE_TEX_ID: u32 = 3101;
const UI2_GLASS_SURFACE_TEX_ID: u32 = 3102;

#[derive(Copy, Clone)]
struct Ui2SurfaceSlot {
    tex_id: u32,
    width: u32,
    height: u32,
    dirty: bool,
    label: &'static str,
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
static UI2_SURFACES: Once<Mutex<Vec<Ui2SurfaceSlot>>> = Once::new();
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
            "Notes",
            Ui2Rect::new(120.0, 92.0, 352.0, 228.0),
            20,
            246,
        );

        let _ = alloc_window(
            &mut state,
            Ui2WindowKind::Overlay,
            "Glass",
            Ui2Rect::new((view_w as f32) - 336.0, 104.0, 272.0, 188.0),
            24,
            224,
        );

        Mutex::new(state)
    })
}

fn surface_state() -> &'static Mutex<Vec<Ui2SurfaceSlot>> {
    UI2_SURFACES.call_once(|| {
        Mutex::new(vec![
            Ui2SurfaceSlot {
                tex_id: UI2_NOTES_SURFACE_TEX_ID,
                width: 0,
                height: 0,
                dirty: true,
                label: "notes",
            },
            Ui2SurfaceSlot {
                tex_id: UI2_GLASS_SURFACE_TEX_ID,
                width: 0,
                height: 0,
                dirty: true,
                label: "glass",
            },
        ])
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

fn window_content_rect(window: &Ui2Window) -> Option<Ui2Rect> {
    let w = (window.rect.w - 2.0).max(1.0);
    let h = (window.rect.h - UI2_TITLE_H - 1.0).max(1.0);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    Some(Ui2Rect::new(
        window.rect.x + 1.0,
        window.rect.y + UI2_TITLE_H,
        w,
        h,
    ))
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

fn surface_tex_id_for_window(window: &Ui2Window) -> Option<u32> {
    match window.kind {
        Ui2WindowKind::Dialog => Some(UI2_NOTES_SURFACE_TEX_ID),
        Ui2WindowKind::Overlay => Some(UI2_GLASS_SURFACE_TEX_ID),
        _ => None,
    }
}

fn surface_slot_mut(slots: &mut [Ui2SurfaceSlot], tex_id: u32) -> Option<&mut Ui2SurfaceSlot> {
    slots.iter_mut().find(|slot| slot.tex_id == tex_id)
}

fn ensure_surface_storage(tex_id: u32, width: u32, height: u32) -> bool {
    let need = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if need == 0 {
        return false;
    }
    let zeros = vec![0u8; need];
    unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
            tex_id,
            width,
            height,
            zeros.as_ptr(),
            zeros.len(),
        ) == 0
    }
}

fn draw_texture_rect_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    view_w: u32,
    view_h: u32,
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
            u: 0.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: bottom,
            u: 1.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: left,
            y: bottom,
            u: 0.0,
            v: 1.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: right,
            y: top,
            u: 1.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Ui2TexVertex {
            x: left,
            y: top,
            u: 0.0,
            v: 0.0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    ];
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
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

fn render_notes_surface(view_w: u32, view_h: u32) {
    let bg = (0xFA, 0xF3, 0xE8, 0xFF);
    let accent = (0xC8, 0x8B, 0x4A, 0xFF);
    let line = (0xC9, 0xBA, 0xA4, 0xFF);
    let chip = (0x24, 0x2A, 0x33, 0xFF);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(0.0, 0.0, view_w as f32, view_h as f32, bg, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(0.0, 0.0, 6.0, view_h as f32, accent, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(18.0, 18.0, (view_w as f32) - 36.0, 30.0, chip, view_w, view_h);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"ui2 / notes surface", 28.0, 24.0, view_w, view_h, 255);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        b"separate offscreen target composed by ui2",
        24.0,
        62.0,
        view_w,
        view_h,
        220,
    );
    for idx in 0..4 {
        let y = 102.0 + (idx as f32 * 28.0);
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(24.0, y + 10.0, (view_w as f32) - 48.0, 1.0, line, view_w, view_h);
    }
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"- browser host stays separate", 24.0, 96.0, view_w, view_h, 210);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"- cursor plane remains external", 24.0, 124.0, view_w, view_h, 210);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"- loadscreen handoff stays external", 24.0, 152.0, view_w, view_h, 210);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"- this surface should survive moves", 24.0, 180.0, view_w, view_h, 210);
}

fn render_glass_surface(view_w: u32, view_h: u32) {
    let clear = (0x00, 0x00, 0x00, 0x00);
    let shell = (0x4D, 0x7A, 0x74, 0x92);
    let band = (0xB8, 0xD9, 0xD3, 0x72);
    let edge = (0xE8, 0xF4, 0xF2, 0xD6);
    let glow = (0xF1, 0xC4, 0x7A, 0xA8);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(0.0, 0.0, view_w as f32, view_h as f32, clear, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(0.0, 0.0, view_w as f32, view_h as f32, shell, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(16.0, 16.0, (view_w as f32) - 32.0, 34.0, band, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(20.0, 64.0, (view_w as f32) - 40.0, 2.0, edge, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(20.0, (view_h as f32) - 36.0, (view_w as f32) - 40.0, 2.0, edge, view_w, view_h);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present((view_w as f32) - 64.0, 20.0, 24.0, 24.0, glow, view_w, view_h);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"glass overlay", 24.0, 22.0, view_w, view_h, 235);
    crate::gfx::text::draw_atlas_text_in_frame_alpha(
        b"alpha texture over ui2 body",
        24.0,
        78.0,
        view_w,
        view_h,
        220,
    );
    crate::gfx::text::draw_atlas_text_in_frame_alpha(b"blend / scissor demo", 24.0, 106.0, view_w, view_h, 210);
}

fn render_surface_content(window: &Ui2Window, view_w: u32, view_h: u32) {
    match window.kind {
        Ui2WindowKind::Dialog => render_notes_surface(view_w, view_h),
        Ui2WindowKind::Overlay => render_glass_surface(view_w, view_h),
        _ => {}
    }
}

fn prepare_window_surface(window: &Ui2Window) -> Option<(u32, Ui2Rect)> {
    let tex_id = surface_tex_id_for_window(window)?;
    let content = window_content_rect(window)?;
    let width = round_to_u32(content.w, 1);
    let height = round_to_u32(content.h, 1);

    let surface_lock = surface_state();
    let mut surfaces = surface_lock.lock();
    let slot = surface_slot_mut(&mut surfaces, tex_id)?;
    let needs_alloc = slot.width != width || slot.height != height;
    if needs_alloc {
        if !ensure_surface_storage(tex_id, width, height) {
            return None;
        }
        slot.width = width;
        slot.height = height;
        slot.dirty = true;
    }

    if slot.dirty {
        crate::log!(
            "ui2: surface-update kind={} tex={} {}x{}\n",
            slot.label,
            slot.tex_id,
            width,
            height
        );
        let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_render_target(tex_id) };
        let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
        render_surface_content(window, width, height);
        let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_render_target() };
        slot.dirty = false;
    }

    Some((tex_id, content))
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
    let border_rgba = (0x08, 0x0A, 0x0F, 0xFF);
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
        UI2_TITLE_H,
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

    if let Some((tex_id, content)) = prepare_window_surface(window) {
        let sx = round_to_u32(content.x.max(0.0), 0);
        let sy = round_to_u32(content.y.max(0.0), 0);
        let sw = round_to_u32(content.w, 1);
        let sh = round_to_u32(content.h, 1);
        let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_scissor(sx, sy, sw, sh) };
        let _ = draw_texture_rect_no_present(
            tex_id,
            content.x,
            content.y,
            content.w,
            content.h,
            state.view_w,
            state.view_h,
        );
        let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_scissor() };
    } else {
        let body_msg: &[u8] = match window.kind {
            Ui2WindowKind::Browser => &b"browser surface; content compositor slot"[..],
            Ui2WindowKind::Dialog => &b"dialog surface; partial invalidation target"[..],
            Ui2WindowKind::Menu => &b"menu surface; independent top-level alpha"[..],
            Ui2WindowKind::Overlay => &b"overlay surface; translucent top-level pass"[..],
        };
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            body_msg,
            window.rect.x + 12.0,
            window.rect.y + UI2_TITLE_H + 10.0,
            state.view_w,
            state.view_h,
            220,
        );
    }
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
        let _ = prepare_window_surface(window);
    }
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
