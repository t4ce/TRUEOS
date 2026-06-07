use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::{
    Ui3Color, Ui3Command, Ui3HitScene, Ui3Node, Ui3NodeId, Ui3NodeKind, Ui3Point,
    Ui3PointerEventKind, Ui3Rect, Ui3TextParam,
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
    hit_scene: Ui3HitScene,
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

    pub fn declare_node(&mut self, id: Ui3NodeId, kind: Ui3NodeKind) {
        let node = self.ensure_node(id, kind.clone());
        node.kind = kind;
    }

    pub fn node(&self, id: Ui3NodeId) -> Option<&Ui3Node> {
        self.nodes.get(&id)
    }

    pub(super) fn nodes(&self) -> &BTreeMap<Ui3NodeId, Ui3Node> {
        &self.nodes
    }

    pub fn last_frame(&self) -> Option<&Ui3RenderFrame> {
        self.last_frame.as_ref()
    }

    pub fn hit_scene(&self) -> &Ui3HitScene {
        &self.hit_scene
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
            Ui3Command::RemoveChild { parent, child } => {
                self.detach_child(parent, child);
            }
            Ui3Command::RemoveFromParent { node } => {
                self.detach_from_parent(node);
            }
            Ui3Command::RemoveChildren { parent } => {
                self.remove_children(parent);
            }
            Ui3Command::SetPosition { node, position } => {
                self.ensure_node(node, Ui3NodeKind::Container).position = position;
            }
            Ui3Command::SetVisible { node, visible } => {
                self.ensure_node(node, Ui3NodeKind::Container).visible = visible;
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
                self.ensure_node(root, Ui3NodeKind::Container);
                let mut frame = Ui3RenderFrame {
                    root,
                    ordered_nodes: Vec::new(),
                };
                let mut visited = Vec::new();
                self.collect_ordered(root, &mut frame.ordered_nodes, &mut visited);
                self.hit_scene = Ui3HitScene::from_ordered_nodes(&self.nodes, &frame.ordered_nodes);
                self.last_frame = Some(frame);
                return self.last_frame.as_ref();
            }
        }
        None
    }

    fn attach(&mut self, parent: Ui3NodeId, child: Ui3NodeId, index: Option<usize>) {
        if parent == child || self.would_create_cycle(parent, child) {
            return;
        }

        self.ensure_node(parent, Ui3NodeKind::Container);
        self.ensure_node(child, Ui3NodeKind::Container);

        let old_parent = self.nodes.get(&child).and_then(|node| node.parent);
        if let Some(old_parent) = old_parent
            && let Some(old_parent_node) = self.nodes.get_mut(&old_parent)
        {
            old_parent_node
                .children
                .retain(|existing| *existing != child);
        }

        let parent_node = self
            .nodes
            .get_mut(&parent)
            .expect("ui3 parent exists after ensure_node");
        parent_node.children.retain(|existing| *existing != child);
        match index {
            Some(index) => {
                let index = index.min(parent_node.children.len());
                parent_node.children.insert(index, child);
            }
            None => parent_node.children.push(child),
        }

        self.nodes
            .get_mut(&child)
            .expect("ui3 child exists after ensure_node")
            .parent = Some(parent);
    }

    fn would_create_cycle(&self, parent: Ui3NodeId, child: Ui3NodeId) -> bool {
        let mut cursor = Some(parent);
        let mut guard = 0usize;
        while let Some(node_id) = cursor {
            if node_id == child {
                return true;
            }
            guard += 1;
            if guard > self.nodes.len() {
                return true;
            }
            cursor = self.nodes.get(&node_id).and_then(|node| node.parent);
        }
        false
    }

    fn detach_child(&mut self, parent: Ui3NodeId, child: Ui3NodeId) {
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.children.retain(|existing| *existing != child);
        }
        if let Some(child_node) = self.nodes.get_mut(&child)
            && child_node.parent == Some(parent)
        {
            child_node.parent = None;
        }
    }

    fn detach_from_parent(&mut self, node: Ui3NodeId) {
        let parent = self.nodes.get(&node).and_then(|node| node.parent);
        if let Some(parent) = parent {
            self.detach_child(parent, node);
        }
    }

    fn remove_children(&mut self, parent: Ui3NodeId) {
        let children = self
            .ensure_node(parent, Ui3NodeKind::Container)
            .children
            .split_off(0);
        for child in children {
            if let Some(child_node) = self.nodes.get_mut(&child)
                && child_node.parent == Some(parent)
            {
                child_node.parent = None;
            }
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

    fn collect_ordered(
        &self,
        node_id: Ui3NodeId,
        out: &mut Vec<Ui3NodeId>,
        visited: &mut Vec<Ui3NodeId>,
    ) {
        if visited.contains(&node_id) {
            return;
        }
        visited.push(node_id);
        let Some(node) = self.nodes.get(&node_id) else {
            return;
        };
        if !node.visible {
            return;
        }
        out.push(node_id);
        for child in &node.children {
            self.collect_ordered(*child, out, visited);
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
