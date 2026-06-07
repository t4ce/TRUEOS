//! UI3: Pixi-style 2D command host.
//!
//! This module is intentionally not wired into boot yet.  It defines the small
//! retained-scene vocabulary observed from the Parse5/Pixi command trace.

#![allow(dead_code)]

mod pixi_host;

use alloc::string::String;
use alloc::vec::Vec;

pub use self::pixi_host::{Ui3GraphicsOp, Ui3PixiHost, Ui3RenderFrame};

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
    RemoveChildren {
        parent: Ui3NodeId,
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
