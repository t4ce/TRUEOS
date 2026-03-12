use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2LayerKind {
    Background,
    Scene,
    Geometry,
    Cursor,
}

impl Ui2LayerKind {
    const ALL: [Self; 4] = [Self::Background, Self::Scene, Self::Geometry, Self::Cursor];

    #[inline]
    const fn bit(self) -> u32 {
        match self {
            Self::Background => 1 << 0,
            Self::Scene => 1 << 1,
            Self::Geometry => 1 << 2,
            Self::Cursor => 1 << 3,
        }
    }

    #[inline]
    const fn name(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Scene => "scene",
            Self::Geometry => "geometry",
            Self::Cursor => "cursor",
        }
    }
}

#[derive(Copy, Clone)]
struct Ui2Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Ui2Rect {
    #[inline]
    const fn full(view_w: u32, view_h: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: view_w as f32,
            h: view_h as f32,
        }
    }
}

#[derive(Copy, Clone)]
enum Ui2Backing {
    None,
    SolidFill { rgba: (u8, u8, u8, u8) },
}

struct Ui2Layer {
    id: usize,
    parent: Option<usize>,
    kind: Ui2LayerKind,
    rect: Ui2Rect,
    z: i16,
    visible: bool,
    backing: Ui2Backing,
    flash_seq: u32,
}

struct Ui2State {
    view_w: u32,
    view_h: u32,
    layers: Vec<Ui2Layer>,
    compose_seq: u32,
}

static UI2_STATE: Once<Mutex<Ui2State>> = Once::new();
static UI2_PENDING_FLASH_MASK: AtomicU32 = AtomicU32::new(0);
static UI2_STARTED: AtomicBool = AtomicBool::new(false);

fn build_default_layers(view_w: u32, view_h: u32) -> Vec<Ui2Layer> {
    let full = Ui2Rect::full(view_w, view_h);
    vec![
        Ui2Layer {
            id: 0,
            parent: None,
            kind: Ui2LayerKind::Background,
            rect: full,
            z: 0,
            visible: true,
            backing: Ui2Backing::SolidFill {
                rgba: (0xF4, 0xF4, 0xF4, 0xFF),
            },
            flash_seq: 0,
        },
        Ui2Layer {
            id: 1,
            parent: None,
            kind: Ui2LayerKind::Scene,
            rect: full,
            z: 1,
            visible: true,
            backing: Ui2Backing::None,
            flash_seq: 0,
        },
        Ui2Layer {
            id: 2,
            parent: None,
            kind: Ui2LayerKind::Geometry,
            rect: full,
            z: 2,
            visible: true,
            backing: Ui2Backing::None,
            flash_seq: 0,
        },
        Ui2Layer {
            id: 3,
            parent: None,
            kind: Ui2LayerKind::Cursor,
            rect: full,
            z: 3,
            visible: true,
            backing: Ui2Backing::None,
            flash_seq: 0,
        },
    ]
}

fn init_state() -> &'static Mutex<Ui2State> {
    UI2_STATE.call_once(|| {
        let (view_w, view_h) = crate::limine::framebuffer_response()
            .and_then(|resp| resp.framebuffers().next())
            .map(|fb| (fb.width() as u32, fb.height() as u32))
            .unwrap_or((1280, 800));
        Mutex::new(Ui2State {
            view_w,
            view_h,
            layers: build_default_layers(view_w, view_h),
            compose_seq: 0,
        })
    })
}

#[inline]
fn layer_index(state: &Ui2State, kind: Ui2LayerKind) -> usize {
    state
        .layers
        .iter()
        .position(|layer| layer.kind == kind)
        .unwrap_or(0)
}

pub fn request_flash_layer(kind: Ui2LayerKind) {
    let _ = UI2_PENDING_FLASH_MASK.fetch_or(kind.bit(), Ordering::AcqRel);
}

pub fn request_flash_all_layers() {
    let mut mask = 0u32;
    for kind in Ui2LayerKind::ALL {
        mask |= kind.bit();
    }
    let _ = UI2_PENDING_FLASH_MASK.fetch_or(mask, Ordering::AcqRel);
}

fn rerender_layer(state: &mut Ui2State, kind: Ui2LayerKind) {
    let idx = layer_index(state, kind);
    let layer = &mut state.layers[idx];
    layer.rect = Ui2Rect::full(state.view_w, state.view_h);
    layer.flash_seq = layer.flash_seq.wrapping_add(1);
    crate::log!("ui2: flash layer={} seq={}\n", kind.name(), layer.flash_seq);
}

fn sorted_sublayers(state: &Ui2State, parent: Option<usize>) -> Vec<usize> {
    let mut sublayers = Vec::new();
    for layer in &state.layers {
        if layer.parent == parent {
            sublayers.push(layer.id);
        }
    }
    sublayers.sort_by(|lhs, rhs| {
        let a = &state.layers[*lhs];
        let b = &state.layers[*rhs];
        match a.z.cmp(&b.z) {
            CmpOrdering::Equal => a.id.cmp(&b.id),
            other => other,
        }
    });
    sublayers
}

fn draw_layer(state: &Ui2State, layer_id: usize, origin_x: f32, origin_y: f32) {
    let layer = &state.layers[layer_id];
    if !layer.visible {
        return;
    }

    let abs_x = origin_x + layer.rect.x;
    let abs_y = origin_y + layer.rect.y;
    if let Ui2Backing::SolidFill { rgba } = layer.backing {
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            abs_x,
            abs_y,
            layer.rect.w,
            layer.rect.h,
            rgba,
            state.view_w,
            state.view_h,
        );
    }

    for child in sorted_sublayers(state, Some(layer_id)) {
        draw_layer(state, child, abs_x, abs_y);
    }
}

fn compose_layers(state: &mut Ui2State) {
    state.compose_seq = state.compose_seq.wrapping_add(1);
    let clear = match state.layers[layer_index(state, Ui2LayerKind::Background)].backing {
        Ui2Backing::SolidFill { rgba } => {
            ((rgba.0 as u32) << 16) | ((rgba.1 as u32) << 8) | (rgba.2 as u32)
        }
        Ui2Backing::None => 0x000000,
    };

    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(clear) };
    for layer_id in sorted_sublayers(state, None) {
        draw_layer(state, layer_id, 0.0, 0.0);
    }
    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
}

fn process_flash_mask(mask: u32) {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    for kind in Ui2LayerKind::ALL {
        if (mask & kind.bit()) != 0 {
            rerender_layer(&mut state, kind);
        }
    }
    compose_layers(&mut state);
}

#[embassy_executor::task]
pub async fn ui2_task() {
    if UI2_STARTED.swap(true, Ordering::SeqCst) {
        crate::log!("ui2: already running\n");
        return;
    }

    crate::gfx::init(crate::limine::framebuffer_response());
    init_state();
    request_flash_all_layers();
    crate::log!("ui2: boot blank scene\n");

    loop {
        let mask = UI2_PENDING_FLASH_MASK.swap(0, Ordering::AcqRel);
        if mask != 0 {
            process_flash_mask(mask);
        }
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
