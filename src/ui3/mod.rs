//! UI3: Pixi-style 2D command host.
//!
//! It defines the small retained-scene vocabulary observed from the Parse5/Pixi
//! command trace and the AP1 Pixi/QJS smoke host.

#![allow(dead_code)]

mod geometry;
mod hit_scene;
mod intel_present;
mod pixi_host;
mod pixi_service;

use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, Once};

pub use self::geometry::{
    Ui3GeometryFrame, Ui3LoweredDraw, Ui3MeshKind, lower_ui3_frame_geometry, push_ui3_rgb_bytes,
};
pub use self::hit_scene::{Ui3HitEntry, Ui3HitKind, Ui3HitScene, Ui3HitTarget};
pub use self::pixi_host::{Ui3GraphicsOp, Ui3PixiHost, Ui3RenderFrame};
pub use self::pixi_service::{
    pixi_service_draw_count, pixi_service_frame_count, pixi_service_op_count,
    pixi_service_pump_count, pixi_service_ready, pixi_service_render_count, pixi_service_task,
};

pub type Ui3NodeId = u32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui3PointerEventKind {
    PointerDown,
    PointerUp,
    PointerMove,
    PointerOver,
    PointerOut,
    PointerUpOutside,
    ContextMenu,
    Unknown,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Ui3Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Ui3Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Ui3Color {
    pub rgba: u32,
    pub alpha: f32,
}

impl Ui3Color {
    pub const WHITE: Self = Self {
        rgba: 0xFFFF_FFFF,
        alpha: 1.0,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ui3TextParam {
    Text(String),
    Fill(Ui3Color),
    FontSizeTier(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ui3Command {
    AddChild {
        parent: Ui3NodeId,
        child: Ui3NodeId,
    },
    AddChildAt {
        parent: Ui3NodeId,
        child: Ui3NodeId,
        index: usize,
    },
    SetChildIndex {
        parent: Ui3NodeId,
        child: Ui3NodeId,
        index: usize,
    },
    RemoveChild {
        parent: Ui3NodeId,
        child: Ui3NodeId,
    },
    RemoveFromParent {
        node: Ui3NodeId,
    },
    RemoveChildren {
        parent: Ui3NodeId,
    },
    SetPosition {
        node: Ui3NodeId,
        position: Ui3Point,
    },
    SetVisible {
        node: Ui3NodeId,
        visible: bool,
    },
    Listen {
        node: Ui3NodeId,
        event: Ui3PointerEventKind,
    },
    RemoveAllListeners {
        node: Ui3NodeId,
    },
    GraphicsClear {
        node: Ui3NodeId,
    },
    GraphicsRect {
        node: Ui3NodeId,
        rect: Ui3Rect,
    },
    GraphicsCircle {
        node: Ui3NodeId,
        center: Ui3Point,
        radius: f32,
    },
    GraphicsMoveTo {
        node: Ui3NodeId,
        to: Ui3Point,
    },
    GraphicsLineTo {
        node: Ui3NodeId,
        to: Ui3Point,
    },
    GraphicsFill {
        node: Ui3NodeId,
        color: Ui3Color,
    },
    GraphicsStroke {
        node: Ui3NodeId,
        color: Ui3Color,
        width: f32,
    },
    Text {
        node: Ui3NodeId,
        params: Vec<Ui3TextParam>,
    },
    Render {
        root: Ui3NodeId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ui3NodeKind {
    Container,
    Graphics,
    Text,
}

#[derive(Clone, Debug)]
pub struct Ui3Node {
    pub id: Ui3NodeId,
    pub kind: Ui3NodeKind,
    pub label: String,
    pub parent: Option<Ui3NodeId>,
    pub position: Ui3Point,
    pub visible: bool,
    pub children: Vec<Ui3NodeId>,
    pub listeners: Vec<Ui3PointerEventKind>,
    pub graphics: Vec<Ui3GraphicsOp>,
    pub text: String,
    pub text_fill: Ui3Color,
}

impl Ui3Node {
    pub fn new(id: Ui3NodeId, kind: Ui3NodeKind, label: String) -> Self {
        Self {
            id,
            kind,
            label,
            parent: None,
            position: Ui3Point::default(),
            visible: true,
            children: Vec::new(),
            listeners: Vec::new(),
            graphics: Vec::new(),
            text: String::new(),
            text_fill: Ui3Color::WHITE,
        }
    }
}

#[derive(Default)]
struct Ui3TruesurferRuntime {
    host: Ui3PixiHost,
    browser_id: u32,
    root_id: Ui3NodeId,
    op_count: u32,
    frame_count: u32,
}

static UI3_TRUESURFER_RUNTIME: Once<Mutex<Ui3TruesurferRuntime>> = Once::new();

fn ui3_truesurfer_runtime() -> &'static Mutex<Ui3TruesurferRuntime> {
    UI3_TRUESURFER_RUNTIME.call_once(|| Mutex::new(Ui3TruesurferRuntime::default()))
}

fn ui3_scene_kind(kind: u32) -> Ui3NodeKind {
    match kind {
        1 => Ui3NodeKind::Graphics,
        2 => Ui3NodeKind::Text,
        _ => Ui3NodeKind::Container,
    }
}

fn ui3_scene_color(rgb: u32, alpha: f32) -> Ui3Color {
    Ui3Color {
        rgba: 0xff00_0000 | (rgb & 0x00ff_ffff),
        alpha,
    }
}

fn ui3_scene_apply(browser_id: u32, command: Ui3Command) -> i32 {
    let mut runtime = ui3_truesurfer_runtime().lock();
    if runtime.browser_id != browser_id {
        runtime.browser_id = browser_id;
        runtime.root_id = 0;
        runtime.op_count = 0;
        runtime.frame_count = 0;
        runtime.host = Ui3PixiHost::new();
    }
    runtime.op_count = runtime.op_count.wrapping_add(1).max(1);
    let frame = runtime.host.apply(command).cloned();
    if let Some(frame) = frame {
        runtime.frame_count = runtime.frame_count.wrapping_add(1).max(1);
        let geometry = lower_ui3_frame_geometry(&runtime.host, &frame);
        let present = self::intel_present::present_ui3_frame_to_intel_primary(&geometry);
        if runtime.frame_count <= 4 {
            crate::log!(
                "ui3-truesurfer: render browser={} root={} ops={} draws={} solid_rects={} meshes={} text={} presented={} fill_descs={} blend_descs={}\n",
                browser_id,
                frame.root,
                runtime.op_count,
                geometry.draws.len(),
                present.solid_rects,
                present.mesh_draws,
                present.text_runs,
                present.presented as u8,
                present.fill_descs,
                present.blend_descs
            );
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_begin(browser_id: u32, root_id: u32) -> i32 {
    let mut runtime = ui3_truesurfer_runtime().lock();
    runtime.browser_id = browser_id;
    runtime.root_id = root_id;
    runtime.op_count = 0;
    runtime.frame_count = 0;
    runtime.host = Ui3PixiHost::new();
    runtime.host.declare_node(root_id, Ui3NodeKind::Container);
    crate::log!("ui3-truesurfer: scene begin browser={} root={}\n", browser_id, root_id);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_node(browser_id: u32, node_id: u32, kind: u32) -> i32 {
    let mut runtime = ui3_truesurfer_runtime().lock();
    if runtime.browser_id != browser_id {
        runtime.browser_id = browser_id;
        runtime.host = Ui3PixiHost::new();
    }
    runtime.host.declare_node(node_id, ui3_scene_kind(kind));
    runtime.op_count = runtime.op_count.wrapping_add(1).max(1);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_add_child(browser_id: u32, parent: u32, child: u32) -> i32 {
    ui3_scene_apply(browser_id, Ui3Command::AddChild { parent, child })
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_position(
    browser_id: u32,
    node_id: u32,
    x: f32,
    y: f32,
) -> i32 {
    ui3_scene_apply(
        browser_id,
        Ui3Command::SetPosition {
            node: node_id,
            position: Ui3Point { x, y },
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_graphics_clear(browser_id: u32, node_id: u32) -> i32 {
    ui3_scene_apply(browser_id, Ui3Command::GraphicsClear { node: node_id })
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_graphics_rect(
    browser_id: u32,
    node_id: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> i32 {
    ui3_scene_apply(
        browser_id,
        Ui3Command::GraphicsRect {
            node: node_id,
            rect: Ui3Rect { x, y, w, h },
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_graphics_fill(
    browser_id: u32,
    node_id: u32,
    rgb: u32,
    alpha: f32,
) -> i32 {
    ui3_scene_apply(
        browser_id,
        Ui3Command::GraphicsFill {
            node: node_id,
            color: ui3_scene_color(rgb, alpha),
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_graphics_stroke(
    browser_id: u32,
    node_id: u32,
    rgb: u32,
    alpha: f32,
    width: f32,
) -> i32 {
    ui3_scene_apply(
        browser_id,
        Ui3Command::GraphicsStroke {
            node: node_id,
            color: ui3_scene_color(rgb, alpha),
            width,
        },
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui3_scene_text(
    browser_id: u32,
    node_id: u32,
    text_ptr: *const u8,
    text_len: usize,
) -> i32 {
    let text = if text_ptr.is_null() || text_len == 0 {
        String::new()
    } else {
        let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    ui3_scene_apply(
        browser_id,
        Ui3Command::Text {
            node: node_id,
            params: Vec::from([Ui3TextParam::Text(text)]),
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_text_fill(
    browser_id: u32,
    node_id: u32,
    rgb: u32,
    alpha: f32,
) -> i32 {
    ui3_scene_apply(
        browser_id,
        Ui3Command::Text {
            node: node_id,
            params: Vec::from([Ui3TextParam::Fill(ui3_scene_color(rgb, alpha))]),
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_scene_render(browser_id: u32, root_id: u32) -> i32 {
    crate::log!("ui3-truesurfer: render request browser={} root={}\n", browser_id, root_id);
    let result = ui3_scene_apply(browser_id, Ui3Command::Render { root: root_id });
    crate::log!(
        "ui3-truesurfer: render request returned browser={} root={} result={}\n",
        browser_id,
        root_id,
        result
    );
    result
}
