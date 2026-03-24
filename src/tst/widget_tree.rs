#![allow(dead_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::Ui2Rect;

const UI2_WIDGET_TREE_DEMO_TEX_ID: u32 = 4_707;
const UI2_WIDGET_TREE_DEMO_W: u32 = 360;
const UI2_WIDGET_TREE_DEMO_H: u32 = 220;
const UI2_WIDGET_TREE_DEMO_X: f32 = 620.0;
const UI2_WIDGET_TREE_DEMO_Y: f32 = 120.0;
const UI2_WIDGET_TREE_DEMO_Z: i16 = 34;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WidgetId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
    Root,
    Div,
    Details,
    Button,
    TextInput,
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetStyle {
    pub bg_rgba: [u8; 4],
    pub border_rgba: [u8; 4],
    pub text_rgba: [u8; 4],
    pub placeholder_rgba: [u8; 4],
    pub pad_x: u16,
    pub pad_y: u16,
    pub border_px: u16,
}

impl Default for WidgetStyle {
    fn default() -> Self {
        Self {
            bg_rgba: [0xF6, 0xF3, 0xEA, 0xFF],
            border_rgba: [0x2C, 0x28, 0x21, 0xFF],
            text_rgba: [0x21, 0x1D, 0x18, 0xFF],
            placeholder_rgba: [0xA8, 0xA0, 0x93, 0xFF],
            pad_x: 8,
            pad_y: 6,
            border_px: 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetTextureRef {
    pub ready_tex_id: Option<u32>,
    pub placeholder_rgba: [u8; 4],
}

#[derive(Clone, Debug)]
pub enum WidgetPayload {
    None,
    Details {
        summary: String,
        open: bool,
    },
    Button {
        label: String,
        pressed: bool,
    },
    TextInput {
        text: String,
        placeholder: String,
        focused: bool,
    },
}

#[derive(Clone, Debug)]
struct WidgetNode {
    id: WidgetId,
    parent: Option<usize>,
    children: Vec<usize>,
    kind: WidgetKind,
    request_rect: Ui2Rect,
    rect: Ui2Rect,
    style: WidgetStyle,
    payload: WidgetPayload,
    texture: Option<WidgetTextureRef>,
    dirty_local: bool,
    dirty_subtree: bool,
    dirty_rect_hint: Option<Ui2Rect>,
}

#[derive(Clone, Debug)]
pub enum WidgetAction {
    ToggleDetails(WidgetId),
    SetDetailsOpen(WidgetId, bool),
    PressButton(WidgetId, bool),
    SetInputText(WidgetId, String),
    SetInputFocus(WidgetId, bool),
    SetRect(WidgetId, Ui2Rect),
    SetTextureReady(WidgetId, Option<u32>),
}

#[derive(Clone, Debug)]
pub enum WidgetPaintItem {
    FillRect {
        rect: Ui2Rect,
        rgba: [u8; 4],
    },
    StrokeRect {
        rect: Ui2Rect,
        rgba: [u8; 4],
        width: u16,
    },
    Text {
        rect: Ui2Rect,
        text: String,
        rgba: [u8; 4],
    },
    Texture {
        rect: Ui2Rect,
        tex_id: u32,
    },
}

#[derive(Clone, Debug, Default)]
pub struct WidgetFrame {
    pub dirty_rects: Vec<Ui2Rect>,
    pub solved_nodes: Vec<WidgetSolvedNode>,
    pub paint_items: Vec<WidgetPaintItem>,
    pub visited_nodes: usize,
    pub culled_nodes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetSolvedNode {
    pub id: WidgetId,
    pub parent: Option<WidgetId>,
    pub kind: WidgetKind,
    pub rect: Ui2Rect,
}

pub struct WidgetTree {
    nodes: Vec<WidgetNode>,
    next_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetDemoIds {
    pub root: WidgetId,
    pub panel: WidgetId,
    pub details: WidgetId,
    pub button: WidgetId,
    pub input: WidgetId,
}

impl WidgetTree {
    pub fn new(root_rect: Ui2Rect) -> Self {
        Self {
            nodes: vec![WidgetNode {
                id: WidgetId(1),
                parent: None,
                children: Vec::new(),
                kind: WidgetKind::Root,
                request_rect: root_rect,
                rect: root_rect,
                style: WidgetStyle::default(),
                payload: WidgetPayload::None,
                texture: None,
                dirty_local: true,
                dirty_subtree: true,
                dirty_rect_hint: Some(root_rect),
            }],
            next_id: 2,
        }
    }

    pub fn root_id(&self) -> WidgetId {
        self.nodes[0].id
    }

    pub fn build_basic_demo(viewport: Ui2Rect) -> (Self, WidgetDemoIds) {
        let mut tree = Self::new(viewport);
        let root = tree.root_id();
        let panel = tree.add_div(
            root,
            Ui2Rect {
                x: 20.0,
                y: 20.0,
                w: (viewport.w - 40.0).max(120.0),
                h: (viewport.h - 40.0).max(120.0),
            },
            WidgetStyle::default(),
        );
        let details = tree.add_details(
            panel,
            Ui2Rect {
                x: 14.0,
                y: 14.0,
                w: 260.0,
                h: 34.0,
            },
            WidgetStyle::default(),
            "advanced options",
            false,
        );
        let button = tree.add_button(
            details,
            Ui2Rect {
                x: 12.0,
                y: 10.0,
                w: 120.0,
                h: 32.0,
            },
            WidgetStyle::default(),
            "apply",
        );
        let input = tree.add_text_input(
            details,
            Ui2Rect {
                x: 12.0,
                y: 50.0,
                w: 220.0,
                h: 34.0,
            },
            WidgetStyle::default(),
            "type filter",
        );
        (
            tree,
            WidgetDemoIds {
                root,
                panel,
                details,
                button,
                input,
            },
        )
    }

    pub fn add_div(&mut self, parent: WidgetId, rect: Ui2Rect, style: WidgetStyle) -> WidgetId {
        self.push_node(
            parent,
            WidgetKind::Div,
            rect,
            style,
            WidgetPayload::None,
            None,
        )
    }

    pub fn add_details(
        &mut self,
        parent: WidgetId,
        rect: Ui2Rect,
        style: WidgetStyle,
        summary: &str,
        open: bool,
    ) -> WidgetId {
        self.push_node(
            parent,
            WidgetKind::Details,
            rect,
            style,
            WidgetPayload::Details {
                summary: String::from(summary),
                open,
            },
            None,
        )
    }

    pub fn add_button(
        &mut self,
        parent: WidgetId,
        rect: Ui2Rect,
        style: WidgetStyle,
        label: &str,
    ) -> WidgetId {
        self.push_node(
            parent,
            WidgetKind::Button,
            rect,
            style,
            WidgetPayload::Button {
                label: String::from(label),
                pressed: false,
            },
            None,
        )
    }

    pub fn add_text_input(
        &mut self,
        parent: WidgetId,
        rect: Ui2Rect,
        style: WidgetStyle,
        placeholder: &str,
    ) -> WidgetId {
        self.push_node(
            parent,
            WidgetKind::TextInput,
            rect,
            style,
            WidgetPayload::TextInput {
                text: String::new(),
                placeholder: String::from(placeholder),
                focused: false,
            },
            None,
        )
    }

    pub fn set_texture_ref(&mut self, id: WidgetId, texture: Option<WidgetTextureRef>) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.nodes[index].texture = texture;
        self.mark_node_dirty(index, Some(self.nodes[index].rect));
        true
    }

    pub fn apply_action(&mut self, action: WidgetAction) -> bool {
        match action {
            WidgetAction::ToggleDetails(id) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                let next_open = match &self.nodes[index].payload {
                    WidgetPayload::Details { open, .. } => !*open,
                    _ => return false,
                };
                self.set_details_open_by_index(index, next_open)
            }
            WidgetAction::SetDetailsOpen(id, open) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                self.set_details_open_by_index(index, open)
            }
            WidgetAction::PressButton(id, pressed) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                match &mut self.nodes[index].payload {
                    WidgetPayload::Button {
                        pressed: node_pressed,
                        ..
                    } => {
                        if *node_pressed == pressed {
                            return true;
                        }
                        *node_pressed = pressed;
                        self.mark_node_dirty(index, Some(self.nodes[index].rect));
                        true
                    }
                    _ => false,
                }
            }
            WidgetAction::SetInputText(id, text) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                match &mut self.nodes[index].payload {
                    WidgetPayload::TextInput {
                        text: node_text, ..
                    } => {
                        if *node_text == text {
                            return true;
                        }
                        *node_text = text;
                        self.mark_node_dirty(index, Some(self.nodes[index].rect));
                        true
                    }
                    _ => false,
                }
            }
            WidgetAction::SetInputFocus(id, focused) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                match &mut self.nodes[index].payload {
                    WidgetPayload::TextInput {
                        focused: node_focused,
                        ..
                    } => {
                        if *node_focused == focused {
                            return true;
                        }
                        *node_focused = focused;
                        self.mark_node_dirty(index, Some(self.nodes[index].rect));
                        true
                    }
                    _ => false,
                }
            }
            WidgetAction::SetRect(id, next_rect) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                let prev = self.nodes[index].rect;
                if self.nodes[index].request_rect == next_rect {
                    return true;
                }
                self.nodes[index].request_rect = next_rect;
                self.mark_node_dirty(index, Some(union_rect(prev, next_rect)));
                true
            }
            WidgetAction::SetTextureReady(id, ready_tex_id) => {
                let Some(index) = self.index_of(id) else {
                    return false;
                };
                let next = self.nodes[index].texture.map(|mut texture| {
                    texture.ready_tex_id = ready_tex_id;
                    texture
                });
                self.nodes[index].texture = next;
                self.mark_node_dirty(index, Some(self.nodes[index].rect));
                true
            }
        }
    }

    pub fn build_frame(&mut self, viewport: Ui2Rect) -> WidgetFrame {
        let mut frame = WidgetFrame::default();
        frame.solved_nodes = self.solve_layout(viewport);
        self.visit_node(0, viewport, true, &mut frame);
        for node in &mut self.nodes {
            node.dirty_local = false;
            node.dirty_subtree = false;
            node.dirty_rect_hint = None;
        }
        frame
    }

    fn push_node(
        &mut self,
        parent: WidgetId,
        kind: WidgetKind,
        rect: Ui2Rect,
        style: WidgetStyle,
        payload: WidgetPayload,
        texture: Option<WidgetTextureRef>,
    ) -> WidgetId {
        let parent_index = self.index_of(parent).unwrap_or(0);
        let id = WidgetId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let node_index = self.nodes.len();
        self.nodes.push(WidgetNode {
            id,
            parent: Some(parent_index),
            children: Vec::new(),
            kind,
            request_rect: rect,
            rect,
            style,
            payload,
            texture,
            dirty_local: true,
            dirty_subtree: true,
            dirty_rect_hint: Some(rect),
        });
        self.nodes[parent_index].children.push(node_index);
        self.mark_node_dirty(parent_index, Some(rect));
        id
    }

    fn index_of(&self, id: WidgetId) -> Option<usize> {
        self.nodes.iter().position(|node| node.id == id)
    }

    fn set_details_open_by_index(&mut self, index: usize, open: bool) -> bool {
        match &mut self.nodes[index].payload {
            WidgetPayload::Details {
                open: node_open, ..
            } => {
                if *node_open == open {
                    return true;
                }
                *node_open = open;
                self.mark_node_dirty(index, Some(self.nodes[index].rect));
                let children = self.nodes[index].children.clone();
                for child in children {
                    self.mark_node_dirty(child, Some(self.nodes[child].rect));
                }
                true
            }
            _ => false,
        }
    }

    fn mark_node_dirty(&mut self, index: usize, rect_hint: Option<Ui2Rect>) {
        let mut current = Some(index);
        let mut local = true;
        while let Some(node_index) = current {
            let node = &mut self.nodes[node_index];
            if local {
                node.dirty_local = true;
                if let Some(rect) = rect_hint {
                    node.dirty_rect_hint = Some(match node.dirty_rect_hint {
                        Some(prev) => union_rect(prev, rect),
                        None => rect,
                    });
                }
            }
            node.dirty_subtree = true;
            current = node.parent;
            local = false;
        }
    }

    fn solve_layout(&mut self, viewport: Ui2Rect) -> Vec<WidgetSolvedNode> {
        let mut solved = Vec::with_capacity(self.nodes.len());
        self.solve_node(0, viewport, &mut solved);
        solved
    }

    fn solve_node(
        &mut self,
        index: usize,
        parent_content: Ui2Rect,
        solved: &mut Vec<WidgetSolvedNode>,
    ) {
        let prev_rect = self.nodes[index].rect;
        let request_rect = self.nodes[index].request_rect;
        let kind = self.nodes[index].kind;
        let style = self.nodes[index].style;
        let details_open = matches!(
            &self.nodes[index].payload,
            WidgetPayload::Details { open: true, .. }
        );

        let mut next_rect = match kind {
            WidgetKind::Root => parent_content,
            _ => Ui2Rect {
                x: parent_content.x + request_rect.x,
                y: parent_content.y + request_rect.y,
                w: request_rect
                    .w
                    .min((parent_content.w - request_rect.x).max(1.0)),
                h: request_rect.h.max(1.0),
            },
        };

        let children = self.nodes[index].children.clone();
        let mut child_content = inset_rect(next_rect, style.pad_x as f32, style.pad_y as f32);
        if kind == WidgetKind::Details {
            child_content.y += 26.0;
            child_content.h = (child_content.h - 26.0).max(0.0);
        }

        if kind == WidgetKind::Root
            || kind == WidgetKind::Div
            || (kind == WidgetKind::Details && details_open)
        {
            for &child in &children {
                self.solve_node(child, child_content, solved);
            }
        }

        if kind == WidgetKind::Details && details_open {
            let mut max_bottom = next_rect.y + 26.0 + style.pad_y as f32;
            for &child in &children {
                let child_rect = self.nodes[child].rect;
                max_bottom = max_bottom.max(child_rect.y + child_rect.h + style.pad_y as f32);
            }
            next_rect.h = next_rect.h.max(max_bottom - next_rect.y);
        }

        if prev_rect != next_rect {
            self.nodes[index].rect = next_rect;
            self.nodes[index].dirty_rect_hint = Some(match self.nodes[index].dirty_rect_hint {
                Some(existing) => union_rect(existing, union_rect(prev_rect, next_rect)),
                None => union_rect(prev_rect, next_rect),
            });
            self.nodes[index].dirty_subtree = true;
        }

        solved.push(WidgetSolvedNode {
            id: self.nodes[index].id,
            parent: self.nodes[index].parent.map(|parent| self.nodes[parent].id),
            kind,
            rect: self.nodes[index].rect,
        });
    }

    fn visit_node(
        &self,
        index: usize,
        viewport: Ui2Rect,
        ancestors_visible: bool,
        frame: &mut WidgetFrame,
    ) {
        let node = &self.nodes[index];
        frame.visited_nodes = frame.visited_nodes.saturating_add(1);

        let self_visible = ancestors_visible && rect_intersects(node.rect, viewport);
        if !self_visible {
            frame.culled_nodes = frame.culled_nodes.saturating_add(1);
            return;
        }

        if node.dirty_local || node.dirty_rect_hint.is_some() {
            push_dirty_rect(
                &mut frame.dirty_rects,
                clip_rect(node.dirty_rect_hint.unwrap_or(node.rect), viewport),
            );
        }

        self.push_node_paint(index, viewport, frame);

        let children_visible = if let WidgetPayload::Details { open, .. } = &node.payload {
            *open
        } else {
            true
        };
        if !children_visible {
            return;
        }

        for &child in &node.children {
            self.visit_node(child, viewport, true, frame);
        }
    }

    fn push_node_paint(&self, index: usize, viewport: Ui2Rect, frame: &mut WidgetFrame) {
        let node = &self.nodes[index];
        let Some(rect) = clip_rect(node.rect, viewport) else {
            return;
        };

        if node.style.bg_rgba[3] > 0 {
            frame.paint_items.push(WidgetPaintItem::FillRect {
                rect,
                rgba: node.style.bg_rgba,
            });
        }

        if let Some(texture) = node.texture {
            match texture.ready_tex_id {
                Some(tex_id) => frame
                    .paint_items
                    .push(WidgetPaintItem::Texture { rect, tex_id }),
                None if texture.placeholder_rgba[3] > 0 => {
                    frame.paint_items.push(WidgetPaintItem::FillRect {
                        rect,
                        rgba: texture.placeholder_rgba,
                    });
                }
                None => {}
            }
        }

        if node.style.border_px > 0 && node.style.border_rgba[3] > 0 {
            frame.paint_items.push(WidgetPaintItem::StrokeRect {
                rect,
                rgba: node.style.border_rgba,
                width: node.style.border_px,
            });
        }

        if let Some(text) = node_paint_text(node) {
            frame.paint_items.push(WidgetPaintItem::Text {
                rect: inset_rect(node.rect, node.style.pad_x as f32, node.style.pad_y as f32),
                text,
                rgba: node_text_rgba(node),
            });
        }
    }
}

fn node_paint_text(node: &WidgetNode) -> Option<String> {
    match &node.payload {
        WidgetPayload::None => None,
        WidgetPayload::Details { summary, open } => Some(if *open {
            alloc::format!("v {}", summary)
        } else {
            alloc::format!("> {}", summary)
        }),
        WidgetPayload::Button { label, .. } => Some(label.clone()),
        WidgetPayload::TextInput {
            text,
            placeholder,
            focused,
        } => {
            if !text.is_empty() {
                Some(if *focused {
                    alloc::format!("{}|", text)
                } else {
                    text.clone()
                })
            } else if *focused {
                Some(String::from("|"))
            } else if !placeholder.is_empty() {
                Some(placeholder.clone())
            } else {
                None
            }
        }
    }
}

fn node_text_rgba(node: &WidgetNode) -> [u8; 4] {
    match &node.payload {
        WidgetPayload::TextInput { text, focused, .. } if text.is_empty() && !*focused => {
            node.style.placeholder_rgba
        }
        WidgetPayload::Button { pressed, .. } if *pressed => [0xFA, 0xFA, 0xF8, 0xFF],
        _ => node.style.text_rgba,
    }
}

fn push_dirty_rect(out: &mut Vec<Ui2Rect>, rect: Option<Ui2Rect>) {
    let Some(rect) = rect else {
        return;
    };
    for existing in out.iter_mut() {
        if rect_intersects(*existing, rect) {
            *existing = union_rect(*existing, rect);
            return;
        }
    }
    out.push(rect);
}

fn rect_intersects(a: Ui2Rect, b: Ui2Rect) -> bool {
    let ax1 = a.x + a.w;
    let ay1 = a.y + a.h;
    let bx1 = b.x + b.w;
    let by1 = b.y + b.h;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

fn clip_rect(a: Ui2Rect, b: Ui2Rect) -> Option<Ui2Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Ui2Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn union_rect(a: Ui2Rect, b: Ui2Rect) -> Ui2Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Ui2Rect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.0),
        h: (y1 - y0).max(0.0),
    }
}

fn inset_rect(rect: Ui2Rect, inset_x: f32, inset_y: f32) -> Ui2Rect {
    Ui2Rect {
        x: rect.x + inset_x,
        y: rect.y + inset_y,
        w: (rect.w - inset_x * 2.0).max(0.0),
        h: (rect.h - inset_y * 2.0).max(0.0),
    }
}

fn fill_pixels_rgba(pixels: &mut [u8], width: u32, height: u32, rect: Ui2Rect, rgba: [u8; 4]) {
    let Some(rect) = clip_rect(
        rect,
        Ui2Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
        },
    ) else {
        return;
    };
    let x0 = rect.x.max(0.0) as u32;
    let y0 = rect.y.max(0.0) as u32;
    let x1 = (rect.x + rect.w).min(width as f32) as u32;
    let y1 = (rect.y + rect.h).min(height as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y as usize) * (width as usize) + x as usize) * 4;
            pixels[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
}

fn stroke_pixels_rgba(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rect: Ui2Rect,
    rgba: [u8; 4],
    border: u16,
) {
    let border = border.max(1) as f32;
    fill_pixels_rgba(
        pixels,
        width,
        height,
        Ui2Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: border,
        },
        rgba,
    );
    fill_pixels_rgba(
        pixels,
        width,
        height,
        Ui2Rect {
            x: rect.x,
            y: rect.y + rect.h - border,
            w: rect.w,
            h: border,
        },
        rgba,
    );
    fill_pixels_rgba(
        pixels,
        width,
        height,
        Ui2Rect {
            x: rect.x,
            y: rect.y,
            w: border,
            h: rect.h,
        },
        rgba,
    );
    fill_pixels_rgba(
        pixels,
        width,
        height,
        Ui2Rect {
            x: rect.x + rect.w - border,
            y: rect.y,
            w: border,
            h: rect.h,
        },
        rgba,
    );
}

fn draw_widget_paint_item(pixels: &mut [u8], width: u32, height: u32, item: &WidgetPaintItem) {
    match item {
        WidgetPaintItem::FillRect { rect, rgba } => {
            fill_pixels_rgba(pixels, width, height, *rect, *rgba);
        }
        WidgetPaintItem::StrokeRect {
            rect,
            rgba,
            width: border,
        } => {
            stroke_pixels_rgba(pixels, width, height, *rect, *rgba, *border);
        }
        WidgetPaintItem::Text { rect, text, rgba } => {
            let bar_w = (text.len() as f32 * 6.0).min(rect.w.max(0.0)).max(4.0);
            fill_pixels_rgba(
                pixels,
                width,
                height,
                Ui2Rect {
                    x: rect.x,
                    y: rect.y + 4.0,
                    w: bar_w,
                    h: (rect.h - 8.0).min(8.0).max(3.0),
                },
                *rgba,
            );
        }
        WidgetPaintItem::Texture { rect, tex_id } => {
            let tint = ((*tex_id % 251) as u8).max(32);
            fill_pixels_rgba(
                pixels,
                width,
                height,
                *rect,
                [tint, tint / 2, 255u8.saturating_sub(tint / 3), 0xFF],
            );
        }
    }
}

fn rasterize_widget_frame(frame: &WidgetFrame, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    fill_pixels_rgba(
        &mut pixels,
        width,
        height,
        Ui2Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
        },
        [0xE8, 0xE2, 0xD5, 0xFF],
    );

    for item in &frame.paint_items {
        draw_widget_paint_item(&mut pixels, width, height, item);
    }

    pixels
}

fn extract_pixels_rgba_region(
    src: &[u8],
    width: u32,
    height: u32,
    region: Ui2Rect,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let x0 = libm::floorf(region.x.max(0.0)) as u32;
    let y0 = libm::floorf(region.y.max(0.0)) as u32;
    let x1 = libm::ceilf(region.x + region.w).min(width as f32) as u32;
    let y1 = libm::ceilf(region.y + region.h).min(height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let row_bytes = (x1 - x0) as usize * 4;
    let row_count = (y1 - y0) as usize;
    let mut out = vec![0u8; row_bytes.saturating_mul(row_count)];
    for row in 0usize..row_count {
        let src_off = ((y0 as usize + row)
            .saturating_mul(width as usize)
            .saturating_add(x0 as usize))
        .saturating_mul(4);
        let dst_off = row.saturating_mul(row_bytes);
        out[dst_off..dst_off + row_bytes].copy_from_slice(&src[src_off..src_off + row_bytes]);
    }
    Some((x0, y0, x1 - x0, y1 - y0, out))
}

fn rerasterize_widget_frame_region(
    pixels: &mut [u8],
    frame: &WidgetFrame,
    width: u32,
    height: u32,
    dirty: Ui2Rect,
) {
    fill_pixels_rgba(pixels, width, height, dirty, [0xE8, 0xE2, 0xD5, 0xFF]);
    for item in &frame.paint_items {
        let item_rect = match item {
            WidgetPaintItem::FillRect { rect, .. } => *rect,
            WidgetPaintItem::StrokeRect { rect, .. } => *rect,
            WidgetPaintItem::Text { rect, .. } => *rect,
            WidgetPaintItem::Texture { rect, .. } => *rect,
        };
        if rect_intersects(item_rect, dirty) {
            draw_widget_paint_item(pixels, width, height, item);
        }
    }
}

#[embassy_executor::task]
pub async fn ui2_widget_tree_demo_task() {
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Widget Tree Demo",
        Ui2Rect {
            x: UI2_WIDGET_TREE_DEMO_X,
            y: UI2_WIDGET_TREE_DEMO_Y,
            w: UI2_WIDGET_TREE_DEMO_W as f32,
            h: UI2_WIDGET_TREE_DEMO_H as f32,
        },
        UI2_WIDGET_TREE_DEMO_Z,
        255,
        UI2_WIDGET_TREE_DEMO_TEX_ID,
        true,
        [0xE8, 0xE2, 0xD5, 0xFF],
    ) else {
        return;
    };

    let viewport = Ui2Rect {
        x: 0.0,
        y: 0.0,
        w: UI2_WIDGET_TREE_DEMO_W as f32,
        h: UI2_WIDGET_TREE_DEMO_H as f32,
    };
    let (mut tree, demo) = WidgetTree::build_basic_demo(viewport);
    let mut pixels =
        vec![0u8; UI2_WIDGET_TREE_DEMO_W as usize * UI2_WIDGET_TREE_DEMO_H as usize * 4];
    let mut step = 0u32;

    loop {
        match step % 6 {
            0 => {
                let _ = tree.apply_action(WidgetAction::ToggleDetails(demo.details));
            }
            1 => {
                let _ = tree.apply_action(WidgetAction::SetInputFocus(demo.input, true));
            }
            2 => {
                let _ = tree.apply_action(WidgetAction::SetInputText(
                    demo.input,
                    format!("delta-{}", step / 6),
                ));
            }
            3 => {
                let _ = tree.apply_action(WidgetAction::PressButton(demo.button, true));
            }
            4 => {
                let _ = tree.apply_action(WidgetAction::PressButton(demo.button, false));
            }
            _ => {
                let _ = tree.apply_action(WidgetAction::SetInputFocus(demo.input, false));
            }
        }

        let frame = tree.build_frame(viewport);
        if !frame.dirty_rects.is_empty() {
            let mut upload_mode = "none";
            let mut upload_rect = (0u32, 0u32, 0u32, 0u32);
            let mut upload_count = 0usize;
            if step == 0 {
                pixels =
                    rasterize_widget_frame(&frame, UI2_WIDGET_TREE_DEMO_W, UI2_WIDGET_TREE_DEMO_H);
                let _ = surface.upload_rgba(&pixels, "widget-tree-demo-init");
                upload_mode = "full";
                upload_rect = (0, 0, UI2_WIDGET_TREE_DEMO_W, UI2_WIDGET_TREE_DEMO_H);
                upload_count = 1;
            } else {
                for dirty in &frame.dirty_rects {
                    rerasterize_widget_frame_region(
                        &mut pixels,
                        &frame,
                        UI2_WIDGET_TREE_DEMO_W,
                        UI2_WIDGET_TREE_DEMO_H,
                        *dirty,
                    );
                    if let Some((x, y, w, h, region_pixels)) = extract_pixels_rgba_region(
                        pixels.as_slice(),
                        UI2_WIDGET_TREE_DEMO_W,
                        UI2_WIDGET_TREE_DEMO_H,
                        *dirty,
                    ) {
                        let _ = surface.upload_rgba_region(
                            x,
                            y,
                            w,
                            h,
                            region_pixels.as_slice(),
                            "widget-tree-demo-region",
                        );
                        upload_mode = "region";
                        upload_rect = (x, y, w, h);
                        upload_count = upload_count.saturating_add(1);
                    }
                }
            }
            crate::log!(
                "ui2-widget-tree-demo: dirty={} paint={} solved={} visited={} culled={} step={} upload={} uploads={} rect={}x{}@{},{}\n",
                frame.dirty_rects.len(),
                frame.paint_items.len(),
                frame.solved_nodes.len(),
                frame.visited_nodes,
                frame.culled_nodes,
                step,
                upload_mode,
                upload_count,
                upload_rect.2,
                upload_rect.3,
                upload_rect.0,
                upload_rect.1
            );
        }

        step = step.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(700)).await;
    }
}
