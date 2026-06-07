use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::{
    Ui3Color, Ui3Command, Ui3Node, Ui3NodeId, Ui3NodeKind, Ui3Point, Ui3PointerEventKind, Ui3Rect,
    Ui3TextParam,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Ui3GraphicsOp {
    Rect(Ui3Rect),
    Circle { center: Ui3Point, radius: f32 },
    MoveTo(Ui3Point),
    LineTo(Ui3Point),
    Fill(Ui3Color),
    Stroke { color: Ui3Color, width: f32 },
}

#[derive(Clone, Debug, Default)]
pub struct Ui3RenderFrame {
    pub root: Ui3NodeId,
    pub ordered_nodes: Vec<Ui3NodeId>,
}

#[derive(Default)]
pub struct Ui3PixiHost {
    nodes: BTreeMap<Ui3NodeId, Ui3Node>,
    last_frame: Option<Ui3RenderFrame>,
}

impl Ui3PixiHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_node(&mut self, id: Ui3NodeId, kind: Ui3NodeKind) -> &mut Ui3Node {
        self.nodes
            .entry(id)
            .or_insert_with(|| Ui3Node::new(id, kind, alloc::format!("ui3-node-{}", id)))
    }

    pub fn node(&self, id: Ui3NodeId) -> Option<&Ui3Node> {
        self.nodes.get(&id)
    }

    pub fn last_frame(&self) -> Option<&Ui3RenderFrame> {
        self.last_frame.as_ref()
    }

    pub fn apply(&mut self, command: Ui3Command) -> Option<&Ui3RenderFrame> {
        match command {
            Ui3Command::AddChild { parent, child } => {
                self.attach(parent, child, None);
            }
            Ui3Command::AddChildAt {
                parent,
                child,
                index,
            } => {
                self.attach(parent, child, Some(index));
            }
            Ui3Command::SetChildIndex {
                parent,
                child,
                index,
            } => {
                self.attach(parent, child, Some(index));
            }
            Ui3Command::RemoveChildren { parent } => {
                self.ensure_node(parent, Ui3NodeKind::Container)
                    .children
                    .clear();
            }
            Ui3Command::Listen { node, event } => {
                let n = self.ensure_node(node, Ui3NodeKind::Container);
                if !n.listeners.contains(&event) {
                    n.listeners.push(event);
                }
            }
            Ui3Command::RemoveAllListeners { node } => {
                self.ensure_node(node, Ui3NodeKind::Container)
                    .listeners
                    .clear();
            }
            Ui3Command::GraphicsClear { node } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .clear();
            }
            Ui3Command::GraphicsRect { node, rect } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::Rect(rect));
            }
            Ui3Command::GraphicsCircle {
                node,
                center,
                radius,
            } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::Circle { center, radius });
            }
            Ui3Command::GraphicsMoveTo { node, to } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::MoveTo(to));
            }
            Ui3Command::GraphicsLineTo { node, to } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::LineTo(to));
            }
            Ui3Command::GraphicsFill { node, color } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::Fill(color));
            }
            Ui3Command::GraphicsStroke { node, color, width } => {
                self.ensure_node(node, Ui3NodeKind::Graphics)
                    .graphics
                    .push(Ui3GraphicsOp::Stroke { color, width });
            }
            Ui3Command::Text { node, params } => {
                self.apply_text(node, params);
            }
            Ui3Command::Render { root } => {
                let mut frame = Ui3RenderFrame {
                    root,
                    ordered_nodes: Vec::new(),
                };
                self.collect_ordered(root, &mut frame.ordered_nodes);
                self.last_frame = Some(frame);
                return self.last_frame.as_ref();
            }
        }
        None
    }

    fn attach(&mut self, parent: Ui3NodeId, child: Ui3NodeId, index: Option<usize>) {
        self.ensure_node(child, Ui3NodeKind::Container);
        let parent_node = self.ensure_node(parent, Ui3NodeKind::Container);
        parent_node.children.retain(|existing| *existing != child);
        match index {
            Some(index) => {
                let index = index.min(parent_node.children.len());
                parent_node.children.insert(index, child);
            }
            None => parent_node.children.push(child),
        }
    }

    fn apply_text(&mut self, node: Ui3NodeId, params: Vec<Ui3TextParam>) {
        let n = self.ensure_node(node, Ui3NodeKind::Text);
        for param in params {
            match param {
                Ui3TextParam::Text(text) => n.text = text,
                Ui3TextParam::Fill(fill) => n.text_fill = fill,
                Ui3TextParam::FontSizeTier(_) => {}
            }
        }
    }

    fn collect_ordered(&self, node_id: Ui3NodeId, out: &mut Vec<Ui3NodeId>) {
        let Some(node) = self.nodes.get(&node_id) else {
            return;
        };
        if !node.visible {
            return;
        }
        out.push(node_id);
        for child in &node.children {
            self.collect_ordered(*child, out);
        }
    }
}

pub const fn pointer_event_kind_from_name(name: &str) -> Ui3PointerEventKind {
    match name.as_bytes() {
        b"pointerdown" => Ui3PointerEventKind::PointerDown,
        b"pointerup" => Ui3PointerEventKind::PointerUp,
        b"pointermove" => Ui3PointerEventKind::PointerMove,
        b"pointerover" => Ui3PointerEventKind::PointerOver,
        b"pointerout" => Ui3PointerEventKind::PointerOut,
        b"pointerupoutside" => Ui3PointerEventKind::PointerUpOutside,
        b"contextmenu" => Ui3PointerEventKind::ContextMenu,
        _ => Ui3PointerEventKind::Unknown,
    }
}
