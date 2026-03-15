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
const UI2_SVG_TEX_ID_BASE: u32 = 3_200;
const UI2_ASYNC_TEX_STATUS_UNKNOWN: i32 = 0;
const UI2_ASYNC_TEX_STATUS_PENDING: i32 = 1;
const UI2_ASYNC_TEX_STATUS_READY: i32 = 2;
static UI2_BROWSER_SNAPSHOT_LOG_SEQ: AtomicU32 = AtomicU32::new(0);
static UI2_SVG_QUEUE_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_SVG_REPAINT_REQUESTED: AtomicBool = AtomicBool::new(false);

struct Ui2SvgFixture {
    tex_id: u32,
    svg: &'static str,
}

#[derive(Copy, Clone)]
struct Ui2SvgAsset {
    tex_id: u32,
    width: u32,
    height: u32,
    status: i32,
}

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
static UI2_SVG_ASSETS: Once<Mutex<Vec<Ui2SvgAsset>>> = Once::new();
static UI2_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_DIRTY: AtomicBool = AtomicBool::new(false);
static UI2_BROWSER_WINDOW_ID: AtomicU32 = AtomicU32::new(0);

const UI2_SVG_FIXTURES: &[Ui2SvgFixture] = &[
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#132a4f"/>
      <stop offset="55%" stop-color="#f26b5b"/>
      <stop offset="100%" stop-color="#ffd27a"/>
    </linearGradient>
    <radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff3bf"/>
      <stop offset="100%" stop-color="#ff9f43"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="url(#sky)"/>
  <circle cx="48" cy="38" r="18" fill="url(#sun)"/>
  <path d="M0 64 C10 58 20 56 32 60 C42 63 54 66 66 62 C78 58 87 59 96 64 L96 96 L0 96 Z" fill="#553c66"/>
  <path d="M0 74 C10 70 20 67 32 70 C42 73 56 76 70 72 C82 68 90 69 96 72 L96 96 L0 96 Z" fill="#2c2348"/>
  <path d="M0 84 C12 80 23 78 34 81 C46 84 58 87 70 84 C81 81 90 82 96 84 L96 96 L0 96 Z" fill="#161126"/>
</svg>"##,
    },
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE + 1,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="petal" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#ff8fb1"/>
      <stop offset="100%" stop-color="#ff4d6d"/>
    </linearGradient>
    <radialGradient id="core" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff4b5"/>
      <stop offset="100%" stop-color="#ffb703"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#fff7ef"/>
  <g fill="url(#petal)" stroke="#7a284a" stroke-width="2" stroke-linejoin="round">
    <path d="M48 18 C60 22 66 31 66 42 C58 45 52 45 48 42 C44 45 38 45 30 42 C30 31 36 22 48 18 Z"/>
    <path d="M78 48 C74 60 65 66 54 66 C51 58 51 52 54 48 C51 44 51 38 54 30 C65 30 74 36 78 48 Z"/>
    <path d="M48 78 C36 74 30 65 30 54 C38 51 44 51 48 54 C52 51 58 51 66 54 C66 65 60 74 48 78 Z"/>
    <path d="M18 48 C22 36 31 30 42 30 C45 38 45 44 42 48 C45 52 45 58 42 66 C31 66 22 60 18 48 Z"/>
  </g>
  <circle cx="48" cy="48" r="10" fill="url(#core)" stroke="#8c5a00" stroke-width="2"/>
</svg>"##,
    },
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE + 2,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#8ff7c8" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#0d3b2a" stop-opacity="0.15"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" rx="12" fill="#091a16"/>
  <circle cx="48" cy="48" r="28" fill="url(#glow)"/>
  <circle cx="48" cy="48" r="12" fill="none" stroke="#7df9c1" stroke-width="2"/>
  <circle cx="48" cy="48" r="24" fill="none" stroke="#4dd9a6" stroke-width="2" stroke-opacity="0.8"/>
  <circle cx="48" cy="48" r="36" fill="none" stroke="#2ca67f" stroke-width="2" stroke-opacity="0.6"/>
  <path d="M48 48 L76 34 A32 32 0 0 1 80 48 Z" fill="#8ff7c8" fill-opacity="0.35"/>
  <path d="M48 14 L48 82 M14 48 L82 48" stroke="#74e7b7" stroke-width="1.5" stroke-linecap="round"/>
  <circle cx="48" cy="48" r="4" fill="#d7fff0"/>
</svg>"##,
    },
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE + 3,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="shell" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#8ec5ff"/>
      <stop offset="100%" stop-color="#2d7ff9"/>
    </linearGradient>
    <linearGradient id="spark" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" fill="#f3f8ff"/>
  <path d="M48 14 L76 28 L76 62 C76 74 64 82 48 86 C32 82 20 74 20 62 L20 28 Z" fill="url(#shell)" stroke="#14439a" stroke-width="3" stroke-linejoin="round"/>
  <path d="M48 26 L66 35 L66 58 C66 66 58 72 48 75 C38 72 30 66 30 58 L30 35 Z" fill="#e9f3ff" fill-opacity="0.35"/>
  <path d="M34 28 C42 24 50 24 58 28 C50 31 42 37 36 48 C33 42 32 35 34 28 Z" fill="url(#spark)"/>
  <path d="M34 54 C38 49 43 46 48 46 C53 46 58 49 62 54 C58 60 53 64 48 66 C43 64 38 60 34 54 Z M43 54 C45 52 46 51 48 51 C50 51 51 52 53 54 C51 56 50 57 48 59 C46 57 45 56 43 54 Z" fill="#ffffff" fill-rule="evenodd"/>
</svg>"##,
    },
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE + 4,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#132238"/>
      <stop offset="100%" stop-color="#214d6b"/>
    </linearGradient>
    <linearGradient id="waveA" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#6ee7f9"/>
      <stop offset="100%" stop-color="#3b82f6"/>
    </linearGradient>
    <linearGradient id="waveB" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#f9a8d4"/>
      <stop offset="100%" stop-color="#f97316"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="14" fill="url(#bg)"/>
  <path d="M8 28 C20 16 34 16 46 28 C58 40 72 40 88 28" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 48 C20 36 34 36 46 48 C58 60 72 60 88 48" fill="none" stroke="url(#waveB)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 68 C20 56 34 56 46 68 C58 80 72 80 88 68" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <circle cx="20" cy="78" r="4" fill="#f8fafc"/>
  <circle cx="48" cy="18" r="3" fill="#f8fafc" fill-opacity="0.8"/>
  <circle cx="76" cy="78" r="4" fill="#f8fafc"/>
</svg>"##,
    },
    Ui2SvgFixture {
        tex_id: UI2_SVG_TEX_ID_BASE + 5,
        svg: r##"
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="head" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff6d6"/>
      <stop offset="100%" stop-color="#ffb347"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#090b1a"/>
  <path d="M20 72 C16 54 20 34 34 24 C46 16 62 16 72 24 C82 32 82 48 72 56 C62 64 46 64 34 56 C24 49 24 38 32 32 C39 27 49 27 56 32" fill="none" stroke="#7dd3fc" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M18 76 C30 68 42 64 54 64 C44 70 32 78 24 88 Z" fill="#7dd3fc" fill-opacity="0.35"/>
  <circle cx="58" cy="34" r="8" fill="url(#head)" stroke="#ffedd5" stroke-width="1.5"/>
  <circle cx="70" cy="22" r="2" fill="#ffffff"/>
  <circle cx="78" cy="30" r="1.5" fill="#ffffff" fill-opacity="0.8"/>
</svg>"##,
    },
];

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

        let demo_x = (view_w as f32) - 336.0;

        let browser_id = alloc_window(
            &mut state,
            Ui2WindowKind::Browser,
            "Browser",
            Ui2Rect::new(72.0, 56.0, (demo_x - 96.0).max(360.0), (view_h as f32) - 112.0),
            10,
            255,
        );
        UI2_BROWSER_WINDOW_ID.store(browser_id, Ordering::Release);

        let _ = alloc_window(
            &mut state,
            Ui2WindowKind::Dialog,
            "SVG List A",
            Ui2Rect::new(demo_x, 72.0, 272.0, 236.0),
            20,
            246,
        );

        let _ = alloc_window(
            &mut state,
            Ui2WindowKind::Overlay,
            "SVG List B",
            Ui2Rect::new(demo_x, 324.0, 272.0, 236.0),
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

fn svg_asset_state() -> &'static Mutex<Vec<Ui2SvgAsset>> {
    UI2_SVG_ASSETS.call_once(|| {
        let mut assets = Vec::with_capacity(UI2_SVG_FIXTURES.len());
        for fixture in UI2_SVG_FIXTURES {
            assets.push(Ui2SvgAsset {
                tex_id: fixture.tex_id,
                width: 0,
                height: 0,
                status: UI2_ASYNC_TEX_STATUS_UNKNOWN,
            });
        }
        Mutex::new(assets)
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

fn mark_demo_surfaces_dirty() {
    let surface_lock = surface_state();
    let mut surfaces = surface_lock.lock();
    for slot in surfaces.iter_mut() {
        slot.dirty = true;
    }
}

fn queue_ui2_svg_assets_once() {
    if UI2_SVG_QUEUE_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let asset_lock = svg_asset_state();
    let mut assets = asset_lock.lock();
    for (index, fixture) in UI2_SVG_FIXTURES.iter().enumerate() {
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_upload_texture_svg_async(
                fixture.tex_id,
                fixture.svg.as_ptr(),
                fixture.svg.len(),
            )
        };
        let status = if rc == 0 {
            UI2_ASYNC_TEX_STATUS_PENDING
        } else {
            rc
        };
        if let Some(asset) = assets.get_mut(index) {
            asset.status = status;
        }
        crate::log!(
            "ui2: svg-queue tex={} idx={} status={}\n",
            fixture.tex_id,
            index,
            status
        );
    }
}

fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    let mut width = 0u32;
    let mut height = 0u32;
    let rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_texture_dimensions(
            tex_id,
            &mut width as *mut u32,
            &mut height as *mut u32,
        )
    };
    if rc == 0 && width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

fn poll_ui2_svg_assets() {
    queue_ui2_svg_assets_once();

    let asset_lock = svg_asset_state();
    let mut assets = asset_lock.lock();
    let mut all_done = !assets.is_empty();

    for asset in assets.iter_mut() {
        if asset.status == UI2_ASYNC_TEX_STATUS_READY || asset.status < 0 {
            continue;
        }

        let status = crate::surface::io::cabi::trueos_cabi_gfx_texture_status(asset.tex_id);
        asset.status = status;
        if status == UI2_ASYNC_TEX_STATUS_READY {
            if let Some((width, height)) = texture_dimensions(asset.tex_id) {
                asset.width = width;
                asset.height = height;
            }
            crate::log!(
                "ui2: svg-ready tex={} {}x{}\n",
                asset.tex_id,
                asset.width,
                asset.height
            );
        } else if status == UI2_ASYNC_TEX_STATUS_PENDING || status == UI2_ASYNC_TEX_STATUS_UNKNOWN {
            all_done = false;
        } else {
            crate::log!("ui2: svg-error tex={} code={}\n", asset.tex_id, status);
        }
    }

    if all_done && !UI2_SVG_REPAINT_REQUESTED.swap(true, Ordering::AcqRel) {
        mark_demo_surfaces_dirty();
        request_full_recompose("ui2-svg-ready");
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

fn render_svg_list_surface(view_w: u32, view_h: u32, start_idx: usize) {
    let bg = (0xF7, 0xF2, 0xEA, 0xFF);
    let strip = (0xD4, 0xC2, 0xAF, 0xFF);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        0.0,
        0.0,
        view_w as f32,
        view_h as f32,
        bg,
        view_w,
        view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        0.0,
        0.0,
        view_w as f32,
        1.0,
        strip,
        view_w,
        view_h,
    );

    let asset_lock = svg_asset_state();
    let assets = asset_lock.lock();
    let pad_x = 14.0;
    let pad_y = 14.0;
    let gap = 12.0;
    let end_idx = core::cmp::min(start_idx + 3, assets.len());
    let visible_count = end_idx.saturating_sub(start_idx);
    if visible_count == 0 {
        return;
    }

    let available_h = ((view_h as f32) - (pad_y * 2.0) - (gap * (visible_count.saturating_sub(1) as f32))).max(1.0);
    let item_h = (available_h / visible_count as f32).max(1.0);
    let item_w = ((view_w as f32) - (pad_x * 2.0)).max(1.0);

    for idx in 0..visible_count {
        let Some(asset) = assets.get(start_idx + idx) else {
            continue;
        };
        if asset.status != UI2_ASYNC_TEX_STATUS_READY || asset.width == 0 || asset.height == 0 {
            continue;
        }

        let scale = libm::fminf(item_w / asset.width as f32, item_h / asset.height as f32);
        let draw_w = (asset.width as f32 * scale).max(1.0);
        let draw_h = (asset.height as f32 * scale).max(1.0);
        let cell_y = pad_y + idx as f32 * (item_h + gap);
        let draw_x = pad_x + ((item_w - draw_w) * 0.5);
        let draw_y = cell_y + ((item_h - draw_h) * 0.5);
        let _ = draw_texture_rect_no_present(
            asset.tex_id,
            draw_x,
            draw_y,
            draw_w,
            draw_h,
            view_w,
            view_h,
            false,
        );
    }
}

fn render_surface_content(window: &Ui2Window, view_w: u32, view_h: u32) {
    match window.kind {
        Ui2WindowKind::Dialog => render_svg_list_surface(view_w, view_h, 0),
        Ui2WindowKind::Overlay => render_svg_list_surface(view_w, view_h, 3),
        _ => {}
    }
}

fn render_surface_target(
    tex_id: u32,
    width: u32,
    height: u32,
    clear_rgb: u32,
    draw: impl FnOnce(u32, u32),
) -> bool {
    let begin_rc = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(clear_rgb) };
    if begin_rc != 0 {
        return false;
    }
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_render_target(tex_id) };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_scissor() };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    draw(width, height);
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_scissor() };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_clear_render_target() };
    let end_rc = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
    end_rc == 0
}

fn ensure_window_surface(window: &Ui2Window) -> Option<(u32, Ui2Rect, bool)> {
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

    Some((tex_id, content, slot.dirty))
}

fn refresh_window_surface(window: &Ui2Window) -> bool {
    let Some((tex_id, _content, dirty)) = ensure_window_surface(window) else {
        return false;
    };
    if !dirty {
        return true;
    }

    let surface_lock = surface_state();
    let mut surfaces = surface_lock.lock();
    let Some(slot) = surface_slot_mut(&mut surfaces, tex_id) else {
        return false;
    };
    let width = slot.width.max(1);
    let height = slot.height.max(1);
    crate::log!(
        "ui2: surface-update kind={} tex={} {}x{}\n",
        slot.label,
        slot.tex_id,
        width,
        height
    );
    if !render_surface_target(tex_id, width, height, 0x000000, |surface_w, surface_h| {
        render_surface_content(window, surface_w, surface_h);
    }) {
        return false;
    }
    slot.dirty = false;
    true
}

fn refresh_dirty_window_surfaces(state: &Ui2State) {
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        let _ = refresh_window_surface(window);
    }
}

fn window_surface_binding(window: &Ui2Window) -> Option<(u32, Ui2Rect)> {
    let (tex_id, content, _dirty) = ensure_window_surface(window)?;
    Some((tex_id, content))
}

fn queue_browser_window_viewport(content: Ui2Rect) {
    let viewport_w = round_to_u32(content.w, 1);
    let viewport_h = round_to_u32(content.h, 1);
    let content_x = libm::roundf(content.x) as i32;
    let content_y = libm::roundf(content.y) as i32;
    let _ = trueos_qjs::browser_task::set_hosted_viewport(
        viewport_w,
        viewport_h,
        content_x,
        content_y,
        viewport_w,
        viewport_h,
    );
}

fn draw_browser_window_content(state: &Ui2State, content: Ui2Rect) -> bool {
    let snapshot = trueos_qjs::browser_task::hosted_surface_state();
    if snapshot.regions.is_empty() || snapshot.viewport_width == 0 || snapshot.viewport_height == 0 {
        return false;
    }

    let last_logged_seq = UI2_BROWSER_SNAPSHOT_LOG_SEQ.load(Ordering::Acquire);
    if last_logged_seq != snapshot.seq {
        UI2_BROWSER_SNAPSHOT_LOG_SEQ.store(snapshot.seq, Ordering::Release);
        crate::log!(
            "ui2: browser-snapshot seq={} viewport={}x{} content_h={} content_top_y={} scroll_y={} regions={}\n",
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
                "ui2: browser-region idx={} tex={} doc_y={} size={}x{} rev={} dirty={}\n",
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

    let sx = round_to_u32(content.x.max(0.0), 0);
    let sy = round_to_u32(content.y.max(0.0), 0);
    let sw = round_to_u32(content.w, 1);
    let sh = round_to_u32(content.h, 1);
    let _ = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_scissor(sx, sy, sw, sh) };

    let draw_w = snapshot.viewport_width.max(1);
    let draw_h = snapshot.viewport_height.max(1);
    let scroll_top = snapshot.content_top_y.saturating_add(snapshot.scroll_y);
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

fn draw_window_frame(state: &Ui2State, window: &Ui2Window) {
    if !window.visible {
        return;
    }

    let frame_rgba = match window.kind {
        Ui2WindowKind::Browser => (0xD9, 0xDE, 0xE5, 0xFF),
        Ui2WindowKind::Dialog => (0xD7, 0xCC, 0xBF, 0xFF),
        Ui2WindowKind::Menu => (0xDD, 0xD2, 0xBD, 0xFF),
        Ui2WindowKind::Overlay => (0xC8, 0xD7, 0xD2, 0xD8),
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
        Ui2WindowKind::Overlay => (0xF2, 0xEA, 0xDE, window.alpha),
    };
    let border_rgba = match window.kind {
        Ui2WindowKind::Browser => (0x9A, 0xA3, 0xAF, 0xFF),
        Ui2WindowKind::Dialog => (0xB8, 0xAA, 0x98, 0xFF),
        Ui2WindowKind::Menu => (0xB8, 0xA6, 0x8A, 0xFF),
        Ui2WindowKind::Overlay => (0x6D, 0x88, 0x80, 0xE0),
    };
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

    if window.kind == Ui2WindowKind::Browser {
        if let Some(content) = window_content_rect(window) {
            queue_browser_window_viewport(content);
            if draw_browser_window_content(state, content) {
                return;
            }
        }
    } else if let Some((tex_id, content)) = window_surface_binding(window) {
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
            false,
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
    svg_asset_state();
    queue_ui2_svg_assets_once();
    request_full_recompose("boot");
    crate::log!("ui2: boot window manager\n");
    let mut last_browser_surface_seq = 0u32;

    loop {
        poll_ui2_svg_assets();
        let next_browser_surface_seq = trueos_qjs::browser_task::hosted_surface_seq();
        if next_browser_surface_seq != last_browser_surface_seq {
            last_browser_surface_seq = next_browser_surface_seq;
            let _ = request_browser_repaint("browser-surface");
        }
        if UI2_DIRTY.swap(false, Ordering::AcqRel) {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            refresh_dirty_window_surfaces(&state);
            compose_windows(&mut state);
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
