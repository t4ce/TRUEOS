use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use parry2d::math::{Pose, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};

use super::{Ui3GraphicsOp, Ui3Node, Ui3NodeId, Ui3NodeKind, Ui3Rect};

const UI3_CURSOR_HIT_RADIUS_PX: f32 = 0.5;

#[derive(Copy, Clone, Debug)]
struct Ui3Transform {
    tx: f32,
    ty: f32,
    sx: f32,
    sy: f32,
}

impl Default for Ui3Transform {
    fn default() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            sx: 1.0,
            sy: 1.0,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Ui3HitKind {
    Listener,
}

#[derive(Copy, Clone, Debug)]
pub struct Ui3HitEntry {
    pub node: Ui3NodeId,
    pub kind: Ui3HitKind,
    pub rect: Ui3Rect,
    pub order: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Ui3HitTarget {
    pub node: Ui3NodeId,
    pub kind: Ui3HitKind,
}

#[derive(Clone, Debug, Default)]
pub struct Ui3HitScene {
    entries: Vec<Ui3HitEntry>,
}

impl Ui3HitScene {
    pub fn from_ordered_nodes(
        nodes: &BTreeMap<Ui3NodeId, Ui3Node>,
        ordered_nodes: &[Ui3NodeId],
    ) -> Self {
        let mut world_bounds = BTreeMap::new();
        for node_id in ordered_nodes.iter().rev().copied() {
            let Some(node) = nodes.get(&node_id) else {
                continue;
            };
            if !node.visible {
                continue;
            }

            let transform = world_transform(nodes, node_id);
            let mut rect = local_node_bounds(node).map(|rect| transform_rect(rect, transform));
            for child_id in &node.children {
                let Some(child_rect) = world_bounds.get(child_id).copied() else {
                    continue;
                };
                rect = union_optional_rect(rect, child_rect);
            }
            if let Some(rect) = rect {
                let rect = match node.mask.and_then(|mask| mask_world_bounds(nodes, mask)) {
                    Some(mask_rect) => intersect_rect(rect, mask_rect),
                    None => Some(rect),
                };
                if let Some(rect) = rect {
                    world_bounds.insert(node_id, rect);
                }
            }
        }

        let mut entries = Vec::new();
        let mut listener_nodes = 0usize;
        let mut listener_with_world_bounds = 0usize;
        let mut listener_missing_world_bounds = 0usize;
        let mut listener_clipped = 0usize;
        let mut sample_node = 0u32;
        let mut sample_kind = 0u32;
        let mut sample_children = 0usize;
        let mut sample_graphics = 0usize;
        let mut sample_text_len = 0usize;
        let mut sample_mask = 0u32;
        for (order, node_id) in ordered_nodes.iter().copied().enumerate() {
            let Some(node) = nodes.get(&node_id) else {
                continue;
            };
            if node.listeners.is_empty() {
                continue;
            }
            listener_nodes = listener_nodes.saturating_add(1);
            if sample_node == 0 {
                sample_node = node_id;
                sample_kind = node_kind_code(&node.kind);
                sample_children = node.children.len();
                sample_graphics = node.graphics.len();
                sample_text_len = node.text.len();
                sample_mask = node.mask.unwrap_or(0);
            }
            let Some(rect) = world_bounds.get(&node_id).copied() else {
                listener_missing_world_bounds = listener_missing_world_bounds.saturating_add(1);
                continue;
            };
            listener_with_world_bounds = listener_with_world_bounds.saturating_add(1);
            let Some(rect) = clipped_by_inherited_masks(nodes, node_id, rect) else {
                listener_clipped = listener_clipped.saturating_add(1);
                continue;
            };
            entries.push(Ui3HitEntry {
                node: node_id,
                kind: Ui3HitKind::Listener,
                rect,
                order: order as u32,
            });
        }

        if listener_nodes > 0 && entries.is_empty() {
            crate::log!(
                "ui3-hit-scene: listeners={} with_world_bounds={} missing_world_bounds={} clipped={} ordered_nodes={} world_bounds={} sample node={} kind={} children={} graphics={} text_len={} mask={}\n",
                listener_nodes,
                listener_with_world_bounds,
                listener_missing_world_bounds,
                listener_clipped,
                ordered_nodes.len(),
                world_bounds.len(),
                sample_node,
                sample_kind,
                sample_children,
                sample_graphics,
                sample_text_len,
                sample_mask
            );
        }

        Self { entries }
    }

    pub fn entries(&self) -> &[Ui3HitEntry] {
        &self.entries
    }

    pub fn hit_at(&self, x: f32, y: f32) -> Option<Ui3HitTarget> {
        for entry in self.entries.iter().rev() {
            if hit_entry_intersects_cursor(entry, x, y) {
                return Some(Ui3HitTarget {
                    node: entry.node,
                    kind: entry.kind,
                });
            }
        }
        None
    }
}

fn node_kind_code(kind: &Ui3NodeKind) -> u32 {
    match kind {
        Ui3NodeKind::Container => 0,
        Ui3NodeKind::Graphics => 1,
        Ui3NodeKind::Text => 2,
    }
}

fn local_node_bounds(node: &Ui3Node) -> Option<Ui3Rect> {
    if let Some(rect) = graphics_bounds(&node.graphics) {
        return Some(rect);
    }
    if !node.text.is_empty() {
        let w = (node.text.len() as f32 * 9.0).max(1.0);
        return Some(Ui3Rect {
            x: 0.0,
            y: 0.0,
            w,
            h: 16.0,
        });
    }

    match node.kind {
        Ui3NodeKind::Graphics => graphics_bounds(&node.graphics),
        Ui3NodeKind::Text => None,
        _ => None,
    }
}

fn graphics_bounds(ops: &[Ui3GraphicsOp]) -> Option<Ui3Rect> {
    let mut rect = None;
    for op in ops {
        match *op {
            Ui3GraphicsOp::Rect(next) => {
                rect = union_optional_rect(rect, next);
            }
            Ui3GraphicsOp::RoundRect { rect: next, .. } => {
                rect = union_optional_rect(rect, next);
            }
            Ui3GraphicsOp::Circle { center, radius } => {
                rect = union_optional_rect(
                    rect,
                    Ui3Rect {
                        x: center.x - radius,
                        y: center.y - radius,
                        w: radius * 2.0,
                        h: radius * 2.0,
                    },
                );
            }
            Ui3GraphicsOp::Ellipse { center, rx, ry } => {
                rect = union_optional_rect(
                    rect,
                    Ui3Rect {
                        x: center.x - rx,
                        y: center.y - ry,
                        w: rx * 2.0,
                        h: ry * 2.0,
                    },
                );
            }
            Ui3GraphicsOp::TextureRect { rect: next, .. } => {
                rect = union_optional_rect(rect, next);
            }
            Ui3GraphicsOp::MoveTo(_)
            | Ui3GraphicsOp::LineTo(_)
            | Ui3GraphicsOp::ClosePath
            | Ui3GraphicsOp::Fill(_)
            | Ui3GraphicsOp::Stroke { .. } => {}
        }
    }
    rect
}

fn union_optional_rect(current: Option<Ui3Rect>, next: Ui3Rect) -> Option<Ui3Rect> {
    if next.w <= 0.0 || next.h <= 0.0 {
        return current;
    }

    Some(match current {
        Some(current) => union_rect(current, next),
        None => next,
    })
}

fn union_rect(a: Ui3Rect, b: Ui3Rect) -> Ui3Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Ui3Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn translate_rect(rect: Ui3Rect, dx: f32, dy: f32) -> Ui3Rect {
    Ui3Rect {
        x: rect.x + dx,
        y: rect.y + dy,
        ..rect
    }
}

fn clipped_by_inherited_masks(
    nodes: &BTreeMap<Ui3NodeId, Ui3Node>,
    node_id: Ui3NodeId,
    rect: Ui3Rect,
) -> Option<Ui3Rect> {
    let mut current = Some(node_id);
    let mut out = rect;
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = nodes.get(&id) else {
            break;
        };
        if let Some(mask_id) = node.mask
            && let Some(mask_rect) = mask_world_bounds(nodes, mask_id)
        {
            out = intersect_rect(out, mask_rect)?;
        }
        current = node.parent;
        depth += 1;
        if depth >= 128 {
            break;
        }
    }
    Some(out)
}

fn mask_world_bounds(nodes: &BTreeMap<Ui3NodeId, Ui3Node>, mask_id: Ui3NodeId) -> Option<Ui3Rect> {
    let node = nodes.get(&mask_id)?;
    let local = graphics_bounds(&node.graphics)?;
    Some(transform_rect(local, world_transform(nodes, mask_id)))
}

fn world_transform(nodes: &BTreeMap<Ui3NodeId, Ui3Node>, node_id: Ui3NodeId) -> Ui3Transform {
    let mut current = Some(node_id);
    let mut chain = Vec::new();
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = nodes.get(&id) else {
            break;
        };
        chain.push(id);
        current = node.parent;
        depth += 1;
        if depth >= 128 {
            break;
        }
    }
    let mut out = Ui3Transform::default();
    for id in chain.iter().rev().copied() {
        let Some(node) = nodes.get(&id) else {
            continue;
        };
        out.tx += node.position.x * out.sx;
        out.ty += node.position.y * out.sy;
        out.sx *= sanitize_scale(node.scale.x);
        out.sy *= sanitize_scale(node.scale.y);
    }
    out
}

fn transform_rect(rect: Ui3Rect, transform: Ui3Transform) -> Ui3Rect {
    let x0 = transform.tx + rect.x * transform.sx;
    let y0 = transform.ty + rect.y * transform.sy;
    let x1 = transform.tx + (rect.x + rect.w) * transform.sx;
    let y1 = transform.ty + (rect.y + rect.h) * transform.sy;
    Ui3Rect {
        x: x0.min(x1),
        y: y0.min(y1),
        w: (x1 - x0).abs(),
        h: (y1 - y0).abs(),
    }
}

fn sanitize_scale(value: f32) -> f32 {
    if value.is_finite() { value } else { 1.0 }
}

fn intersect_rect(a: Ui3Rect, b: Ui3Rect) -> Option<Ui3Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Ui3Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn hit_entry_intersects_cursor(entry: &Ui3HitEntry, cursor_x: f32, cursor_y: f32) -> bool {
    let cursor = Ball::new(UI3_CURSOR_HIT_RADIUS_PX.max(0.5));
    let rect =
        Cuboid::new(Vector::new((entry.rect.w * 0.5).max(0.5), (entry.rect.h * 0.5).max(0.5)));
    let cursor_iso = Pose::translation(cursor_x, cursor_y);
    let rect_iso =
        Pose::translation(entry.rect.x + (entry.rect.w * 0.5), entry.rect.y + (entry.rect.h * 0.5));
    matches!(query::intersection_test(&cursor_iso, &cursor, &rect_iso, &rect), Ok(true))
}
