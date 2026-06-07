use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use parry2d::math::{Pose, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};

use super::{Ui3GraphicsOp, Ui3Node, Ui3NodeId, Ui3NodeKind, Ui3Rect};

const UI3_CURSOR_HIT_RADIUS_PX: f32 = 0.5;

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
        let mut bounds = BTreeMap::new();
        for node_id in ordered_nodes.iter().rev().copied() {
            let Some(node) = nodes.get(&node_id) else {
                continue;
            };
            if !node.visible {
                continue;
            }

            let mut rect = local_node_bounds(node);
            for child_id in &node.children {
                let Some(child) = nodes.get(child_id) else {
                    continue;
                };
                let Some(child_rect) = bounds.get(child_id).copied() else {
                    continue;
                };
                let child_rect = translate_rect(child_rect, child.position.x, child.position.y);
                rect = union_optional_rect(rect, child_rect);
            }
            if let Some(rect) = rect {
                bounds.insert(node_id, rect);
            }
        }

        let mut entries = Vec::new();
        for (order, node_id) in ordered_nodes.iter().copied().enumerate() {
            let Some(node) = nodes.get(&node_id) else {
                continue;
            };
            if node.listeners.is_empty() {
                continue;
            }
            let Some(rect) = bounds.get(&node_id).copied() else {
                continue;
            };
            entries.push(Ui3HitEntry {
                node: node_id,
                kind: Ui3HitKind::Listener,
                rect,
                order: order as u32,
            });
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

fn local_node_bounds(node: &Ui3Node) -> Option<Ui3Rect> {
    match node.kind {
        Ui3NodeKind::Graphics => graphics_bounds(&node.graphics),
        Ui3NodeKind::Text if !node.text.is_empty() => {
            let w = (node.text.len() as f32 * 9.0).max(1.0);
            Some(Ui3Rect {
                x: 0.0,
                y: 0.0,
                w,
                h: 16.0,
            })
        }
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
            Ui3GraphicsOp::MoveTo(_)
            | Ui3GraphicsOp::LineTo(_)
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

fn hit_entry_intersects_cursor(entry: &Ui3HitEntry, cursor_x: f32, cursor_y: f32) -> bool {
    let cursor = Ball::new(UI3_CURSOR_HIT_RADIUS_PX.max(0.5));
    let rect =
        Cuboid::new(Vector::new((entry.rect.w * 0.5).max(0.5), (entry.rect.h * 0.5).max(0.5)));
    let cursor_iso = Pose::translation(cursor_x, cursor_y);
    let rect_iso =
        Pose::translation(entry.rect.x + (entry.rect.w * 0.5), entry.rect.y + (entry.rect.h * 0.5));
    matches!(query::intersection_test(&cursor_iso, &cursor, &rect_iso, &rect), Ok(true))
}
