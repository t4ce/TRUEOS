//! UI3: Pixi-style 2D command host.
//!
//! It defines the retained-scene vocabulary consumed from the TrueSurfer/Pixi
//! command stream and owns composition, hit routing, and presentation.

#![allow(dead_code)]

mod debug_capture;
mod geometry;
mod font {
    pub(super) mod gpgpu_font;
}
mod hit_scene;
mod html_widgets;
mod intel_present;
mod pixi_host;
mod pixi_service;
mod ui3_asset_service;

use alloc::string::String;
use alloc::vec::Vec;

pub(crate) use self::debug_capture::{Ui3DebugCaptureError, latest_pixi_primary_bmp};
pub use self::geometry::{
    Ui3GeometryFrame, Ui3LoweredDraw, Ui3MeshKind, Ui3SolidRectKind, lower_ui3_frame_geometry,
    push_ui3_rgb_bytes,
};
pub use self::hit_scene::{Ui3HitEntry, Ui3HitKind, Ui3HitScene, Ui3HitTarget};
pub use self::pixi_host::{Ui3GraphicsOp, Ui3PixiHost, Ui3RenderFrame};
pub use self::pixi_service::{
    pixi_service_draw_count, pixi_service_filtered_op_count, pixi_service_frame_count,
    pixi_service_op_count, pixi_service_pump_count, pixi_service_ready, pixi_service_render_count,
    pixi_service_task,
};
pub use self::ui3_asset_service::{ui3_asset_service_ready, ui3_asset_service_task};

pub type Ui3NodeId = u32;

pub(crate) const TRUESURFER_SMOKE_HTML_URL: &str = "inline://trueos/parse5-demo-input.html";
pub(crate) const TRUESURFER_SMOKE_HTML_SOURCE: &str = include_str!("truesurfer_demo_input.html");

#[inline]
fn now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui3PointerEventKind {
    PointerDown,
    PointerUp,
    PointerMove,
    PointerOver,
    PointerOut,
    PointerUpOutside,
    ContextMenu,
    Wheel,
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
    FontTier(u8),
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
    SetAlpha {
        node: Ui3NodeId,
        alpha: f32,
    },
    SetScale {
        node: Ui3NodeId,
        scale: Ui3Point,
    },
    SetMask {
        node: Ui3NodeId,
        mask: Option<Ui3NodeId>,
    },
    SetHitArea {
        node: Ui3NodeId,
        rect: Option<Ui3Rect>,
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
    GraphicsRoundRect {
        node: Ui3NodeId,
        rect: Ui3Rect,
        radius: f32,
    },
    GraphicsCircle {
        node: Ui3NodeId,
        center: Ui3Point,
        radius: f32,
    },
    GraphicsEllipse {
        node: Ui3NodeId,
        center: Ui3Point,
        rx: f32,
        ry: f32,
    },
    GraphicsMoveTo {
        node: Ui3NodeId,
        to: Ui3Point,
    },
    GraphicsLineTo {
        node: Ui3NodeId,
        to: Ui3Point,
    },
    GraphicsClosePath {
        node: Ui3NodeId,
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
    TextureRect {
        node: Ui3NodeId,
        tex_id: u32,
        rect: Ui3Rect,
        alpha: f32,
    },
    Text {
        node: Ui3NodeId,
        params: Vec<Ui3TextParam>,
    },
    Render {
        root: Ui3NodeId,
    },
}

fn ui3_light_filter_reason(command: &Ui3Command) -> Option<&'static str> {
    let _ = command;
    None
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
    pub scale: Ui3Point,
    pub visible: bool,
    pub alpha: f32,
    pub mask: Option<Ui3NodeId>,
    pub hit_area: Option<Ui3Rect>,
    pub children: Vec<Ui3NodeId>,
    pub listeners: Vec<Ui3PointerEventKind>,
    pub graphics: Vec<Ui3GraphicsOp>,
    pub text: String,
    pub text_fill: Ui3Color,
    pub text_font_tier: u8,
}

impl Ui3Node {
    pub fn new(id: Ui3NodeId, kind: Ui3NodeKind, label: String) -> Self {
        Self {
            id,
            kind,
            label,
            parent: None,
            position: Ui3Point::default(),
            scale: Ui3Point { x: 1.0, y: 1.0 },
            visible: true,
            alpha: 1.0,
            mask: None,
            hit_area: None,
            children: Vec::new(),
            listeners: Vec::new(),
            graphics: Vec::new(),
            text: String::new(),
            text_fill: Ui3Color::WHITE,
            text_font_tier: 1,
        }
    }
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui3_pixi_op(
    browser_id: u32,
    op_code: u32,
    node: u32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    text_ptr: *const u8,
    text_len: usize,
) -> i32 {
    match op_code {
        0 => pixi_service::queue_scene_begin(browser_id, node),
        1 => pixi_service::queue_scene_node(browser_id, node, ui3_scene_kind(a.max(0.0) as u32)),
        2 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::AddChild {
                parent: node,
                child: a.max(0.0) as u32,
            },
        ),
        3 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetPosition {
                node,
                position: Ui3Point { x: a, y: b },
            },
        ),
        4 => pixi_service::queue_scene_command(browser_id, Ui3Command::GraphicsClear { node }),
        5 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsRect {
                node,
                rect: Ui3Rect {
                    x: a,
                    y: b,
                    w: c,
                    h: d,
                },
            },
        ),
        24 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsRoundRect {
                node,
                rect: Ui3Rect {
                    x: a,
                    y: b,
                    w: c,
                    h: d,
                },
                radius: if text_ptr.is_null() || text_len == 0 {
                    0.0
                } else {
                    let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
                    String::from_utf8_lossy(bytes).parse::<f32>().unwrap_or(0.0)
                },
            },
        ),
        6 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsFill {
                node,
                color: ui3_scene_color(a.max(0.0) as u32, b),
            },
        ),
        7 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsStroke {
                node,
                color: ui3_scene_color(a.max(0.0) as u32, b),
                width: c,
            },
        ),
        8 => {
            let text = if text_ptr.is_null() || text_len == 0 {
                String::new()
            } else {
                let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
                String::from_utf8_lossy(bytes).into_owned()
            };
            pixi_service::queue_scene_command(
                browser_id,
                Ui3Command::Text {
                    node,
                    params: Vec::from([Ui3TextParam::Text(text)]),
                },
            )
        }
        9 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::Text {
                node,
                params: Vec::from([Ui3TextParam::Fill(ui3_scene_color(a.max(0.0) as u32, b))]),
            },
        ),
        30 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::Text {
                node,
                params: Vec::from([Ui3TextParam::FontTier(a.max(0.0).min(2.0) as u8)]),
            },
        ),
        29 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetHitArea {
                node,
                rect: (c > 0.0 && d > 0.0).then_some(Ui3Rect {
                    x: a,
                    y: b,
                    w: c,
                    h: d,
                }),
            },
        ),
        10 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::AddChildAt {
                parent: node,
                child: a.max(0.0) as u32,
                index: b.max(0.0) as usize,
            },
        ),
        11 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetChildIndex {
                parent: node,
                child: a.max(0.0) as u32,
                index: b.max(0.0) as usize,
            },
        ),
        12 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::RemoveChild {
                parent: node,
                child: a.max(0.0) as u32,
            },
        ),
        13 => pixi_service::queue_scene_command(browser_id, Ui3Command::RemoveFromParent { node }),
        14 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::RemoveChildren { parent: node },
        ),
        15 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetVisible {
                node,
                visible: a != 0.0,
            },
        ),
        23 => {
            pixi_service::queue_scene_command(browser_id, Ui3Command::SetAlpha { node, alpha: a })
        }
        28 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetScale {
                node,
                scale: Ui3Point { x: a, y: b },
            },
        ),
        27 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::SetMask {
                node,
                mask: if a > 0.0 { Some(a as u32) } else { None },
            },
        ),
        16 => {
            let event = if text_ptr.is_null() || text_len == 0 {
                String::new()
            } else {
                let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
                String::from_utf8_lossy(bytes).into_owned()
            };
            pixi_service::queue_scene_command(
                browser_id,
                Ui3Command::Listen {
                    node,
                    event: self::pixi_host::pointer_event_kind_from_name(event.as_str()),
                },
            )
        }
        17 => {
            pixi_service::queue_scene_command(browser_id, Ui3Command::RemoveAllListeners { node })
        }
        18 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsCircle {
                node,
                center: Ui3Point { x: a, y: b },
                radius: c,
            },
        ),
        26 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsEllipse {
                node,
                center: Ui3Point { x: a, y: b },
                rx: c,
                ry: d,
            },
        ),
        19 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsMoveTo {
                node,
                to: Ui3Point { x: a, y: b },
            },
        ),
        20 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::GraphicsLineTo {
                node,
                to: Ui3Point { x: a, y: b },
            },
        ),
        25 => pixi_service::queue_scene_command(browser_id, Ui3Command::GraphicsClosePath { node }),
        21 => {
            let rc =
                pixi_service::queue_scene_command(browser_id, Ui3Command::Render { root: node });
            if rc >= 0 {
                let _ = pixi_service::flush_service_queue(8192);
            }
            rc
        }
        22 => pixi_service::queue_scene_command(
            browser_id,
            Ui3Command::TextureRect {
                node,
                tex_id: a.max(0.0) as u32,
                rect: Ui3Rect {
                    x: b,
                    y: c,
                    w: d,
                    h: if text_ptr.is_null() || text_len == 0 {
                        d
                    } else {
                        let bytes = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
                        String::from_utf8_lossy(bytes).parse::<f32>().unwrap_or(d)
                    },
                },
                alpha: 1.0,
            },
        ),
        _ => -1,
    }
}
