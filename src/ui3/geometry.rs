use alloc::string::String;
use alloc::vec::Vec;

use lyon_geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use trueos_gfx_core::{
    RgbVertexPx, Rgba8, ViewTransform, push_indexed_rgb_mesh_px, push_rgb_quad_px,
};
use trueos_math::{cos_f32, sin_f32};

use super::{
    Ui3Color, Ui3GraphicsOp, Ui3Node, Ui3NodeId, Ui3NodeKind, Ui3PixiHost, Ui3Point, Ui3Rect,
    Ui3RenderFrame,
};

const UI3_CIRCLE_SEGMENTS: usize = 32;
const UI3_DEFAULT_STROKE_WIDTH: f32 = 1.0;

#[derive(Clone, Debug, Default)]
pub struct Ui3GeometryFrame {
    pub root: Ui3NodeId,
    pub draws: Vec<Ui3LoweredDraw>,
}

#[derive(Clone, Debug)]
pub enum Ui3LoweredDraw {
    SolidRect {
        node: Ui3NodeId,
        rect: Ui3Rect,
        color: Rgba8,
        kind: Ui3SolidRectKind,
    },
    Mesh {
        node: Ui3NodeId,
        kind: Ui3MeshKind,
        vertices: Vec<RgbVertexPx>,
        indices: Vec<u16>,
    },
    TextRun {
        node: Ui3NodeId,
        origin: Ui3Point,
        text: String,
        color: Rgba8,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui3SolidRectKind {
    Fill,
    RectStroke,
    AxisLineStroke,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui3MeshKind {
    Fill,
    Stroke,
}

#[derive(Copy, Clone, Debug)]
struct CirclePath {
    center: Ui3Point,
    radius: f32,
}

#[derive(Clone, Debug, Default)]
struct PendingPath {
    rects: Vec<Ui3Rect>,
    circles: Vec<CirclePath>,
    subpaths: Vec<Vec<Ui3Point>>,
}

impl PendingPath {
    fn is_empty(&self) -> bool {
        self.rects.is_empty() && self.circles.is_empty() && self.subpaths.is_empty()
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.circles.clear();
        self.subpaths.clear();
    }
}

pub fn lower_ui3_frame_geometry(host: &Ui3PixiHost, frame: &Ui3RenderFrame) -> Ui3GeometryFrame {
    let mut out = Ui3GeometryFrame {
        root: frame.root,
        draws: Vec::new(),
    };

    for node_id in &frame.ordered_nodes {
        let Some(node) = host.node(*node_id) else {
            continue;
        };
        let origin = world_position(host, *node_id);
        match node.kind {
            Ui3NodeKind::Graphics => lower_graphics_node(node, origin, &mut out.draws),
            Ui3NodeKind::Text if !node.text.is_empty() => out.draws.push(Ui3LoweredDraw::TextRun {
                node: node.id,
                origin,
                text: node.text.clone(),
                color: color_to_rgba8(node.text_fill),
            }),
            Ui3NodeKind::Container | Ui3NodeKind::Text => {}
        }
    }

    out
}

pub fn push_ui3_rgb_bytes(frame: &Ui3GeometryFrame, view_w: u32, view_h: u32, out: &mut Vec<u8>) {
    let transform = ViewTransform::from_extent(view_w, view_h);
    for draw in &frame.draws {
        match draw {
            Ui3LoweredDraw::SolidRect { rect, color, .. } => {
                push_rgb_quad_px(
                    out,
                    transform,
                    rect.x,
                    rect.y,
                    rect.x + rect.w,
                    rect.y + rect.h,
                    *color,
                );
            }
            Ui3LoweredDraw::Mesh {
                vertices, indices, ..
            } => {
                push_indexed_rgb_mesh_px(out, transform, vertices, indices);
            }
            Ui3LoweredDraw::TextRun { .. } => {}
        }
    }
}

fn lower_graphics_node(node: &Ui3Node, origin: Ui3Point, draws: &mut Vec<Ui3LoweredDraw>) {
    let mut pending = PendingPath::default();
    let mut last_painted = PendingPath::default();

    for op in &node.graphics {
        match *op {
            Ui3GraphicsOp::Rect(rect) => pending.rects.push(translate_rect(rect, origin)),
            Ui3GraphicsOp::Circle { center, radius } => pending.circles.push(CirclePath {
                center: translate_point(center, origin),
                radius,
            }),
            Ui3GraphicsOp::MoveTo(to) => pending
                .subpaths
                .push(Vec::from([translate_point(to, origin)])),
            Ui3GraphicsOp::LineTo(to) => push_line_to(&mut pending, translate_point(to, origin)),
            Ui3GraphicsOp::Fill(color) => {
                if !pending.is_empty() {
                    emit_fill(node.id, &pending, color_to_rgba8(color), draws);
                    last_painted = pending.clone();
                    pending.clear();
                }
            }
            Ui3GraphicsOp::Stroke { color, width } => {
                let stroke_path = if pending.is_empty() {
                    &last_painted
                } else {
                    &pending
                };
                if !stroke_path.is_empty() {
                    emit_stroke(
                        node.id,
                        stroke_path,
                        color_to_rgba8(color),
                        width.max(UI3_DEFAULT_STROKE_WIDTH),
                        draws,
                    );
                    if !pending.is_empty() {
                        last_painted = pending.clone();
                        pending.clear();
                    }
                }
            }
        }
    }
}

fn emit_fill(node: Ui3NodeId, path: &PendingPath, color: Rgba8, draws: &mut Vec<Ui3LoweredDraw>) {
    for rect in &path.rects {
        draws.push(Ui3LoweredDraw::SolidRect {
            node,
            rect: *rect,
            color,
            kind: Ui3SolidRectKind::Fill,
        });
    }

    for circle in &path.circles {
        if let Some((vertices, indices)) = tessellate_fill_path(&circle_path(*circle), color) {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Fill,
                vertices,
                indices,
            });
        }
    }

    if let Some((vertices, indices)) =
        tessellate_fill_path(&subpaths_to_path(&path.subpaths, true), color)
    {
        draws.push(Ui3LoweredDraw::Mesh {
            node,
            kind: Ui3MeshKind::Fill,
            vertices,
            indices,
        });
    }
}

fn emit_stroke(
    node: Ui3NodeId,
    path: &PendingPath,
    color: Rgba8,
    width: f32,
    draws: &mut Vec<Ui3LoweredDraw>,
) {
    for rect in &path.rects {
        emit_rect_stroke(node, *rect, color, width, draws);
    }

    for circle in &path.circles {
        if let Some((vertices, indices)) =
            tessellate_stroke_path(&circle_path(*circle), color, width)
        {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Stroke,
                vertices,
                indices,
            });
        }
    }

    let fallback_subpaths = emit_axis_aligned_stroke_subpaths(node, &path.subpaths, color, width, draws);
    if let Some((vertices, indices)) =
        tessellate_stroke_path(&subpaths_to_path(&fallback_subpaths, false), color, width)
    {
        draws.push(Ui3LoweredDraw::Mesh {
            node,
            kind: Ui3MeshKind::Stroke,
            vertices,
            indices,
        });
    }
}

fn emit_axis_aligned_stroke_subpaths(
    node: Ui3NodeId,
    subpaths: &[Vec<Ui3Point>],
    color: Rgba8,
    width: f32,
    draws: &mut Vec<Ui3LoweredDraw>,
) -> Vec<Vec<Ui3Point>> {
    let mut fallback = Vec::new();
    for subpath in subpaths {
        if subpath.len() < 2 {
            continue;
        }
        if emit_axis_aligned_stroke_subpath(node, subpath, color, width, draws) {
            continue;
        }
        fallback.push(subpath.clone());
    }
    fallback
}

fn emit_axis_aligned_stroke_subpath(
    node: Ui3NodeId,
    subpath: &[Ui3Point],
    color: Rgba8,
    width: f32,
    draws: &mut Vec<Ui3LoweredDraw>,
) -> bool {
    let mut rects = Vec::new();
    for pair in subpath.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let Some(rect) = axis_aligned_segment_rect(a, b, width) else {
            return false;
        };
        rects.push(rect);
    }

    for rect in rects {
        draws.push(Ui3LoweredDraw::SolidRect {
            node,
            rect,
            color,
            kind: Ui3SolidRectKind::AxisLineStroke,
        });
    }
    true
}

fn axis_aligned_segment_rect(a: Ui3Point, b: Ui3Point, width: f32) -> Option<Ui3Rect> {
    if !a.x.is_finite() || !a.y.is_finite() || !b.x.is_finite() || !b.y.is_finite() {
        return None;
    }
    let w = width.max(UI3_DEFAULT_STROKE_WIDTH);
    let half = w * 0.5;
    if nearly_equal(a.y, b.y) {
        let x0 = a.x.min(b.x);
        let x1 = a.x.max(b.x);
        if x1 <= x0 {
            return None;
        }
        return Some(Ui3Rect {
            x: x0,
            y: a.y - half,
            w: x1 - x0,
            h: w,
        });
    }
    if nearly_equal(a.x, b.x) {
        let y0 = a.y.min(b.y);
        let y1 = a.y.max(b.y);
        if y1 <= y0 {
            return None;
        }
        return Some(Ui3Rect {
            x: a.x - half,
            y: y0,
            w,
            h: y1 - y0,
        });
    }
    None
}

fn nearly_equal(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.01
}

fn emit_rect_stroke(
    node: Ui3NodeId,
    rect: Ui3Rect,
    color: Rgba8,
    width: f32,
    draws: &mut Vec<Ui3LoweredDraw>,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let w = width.max(UI3_DEFAULT_STROKE_WIDTH).min(rect.w.max(rect.h));
    let right = rect.x + rect.w;
    let bottom = rect.y + rect.h;
    let spans = [
        Ui3Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: w.min(rect.h),
        },
        Ui3Rect {
            x: rect.x,
            y: (bottom - w).max(rect.y),
            w: rect.w,
            h: w.min(rect.h),
        },
        Ui3Rect {
            x: rect.x,
            y: rect.y,
            w: w.min(rect.w),
            h: rect.h,
        },
        Ui3Rect {
            x: (right - w).max(rect.x),
            y: rect.y,
            w: w.min(rect.w),
            h: rect.h,
        },
    ];
    for span in spans {
        draws.push(Ui3LoweredDraw::SolidRect {
            node,
            rect: span,
            color,
            kind: Ui3SolidRectKind::RectStroke,
        });
    }
}

fn tessellate_fill_path(path: &Path, color: Rgba8) -> Option<(Vec<RgbVertexPx>, Vec<u16>)> {
    let mut buffers: VertexBuffers<RgbVertexPx, u16> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            path,
            &FillOptions::default(),
            &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
                let p = vertex.position();
                RgbVertexPx {
                    x: p.x,
                    y: p.y,
                    color,
                }
            }),
        )
        .ok()?;
    if buffers.indices.is_empty() {
        return None;
    }
    Some((buffers.vertices, buffers.indices))
}

fn tessellate_stroke_path(
    path: &Path,
    color: Rgba8,
    width: f32,
) -> Option<(Vec<RgbVertexPx>, Vec<u16>)> {
    let mut buffers: VertexBuffers<RgbVertexPx, u16> = VertexBuffers::new();
    StrokeTessellator::new()
        .tessellate_path(
            path,
            &StrokeOptions::default()
                .with_line_width(width.max(UI3_DEFAULT_STROKE_WIDTH))
                .with_line_join(LineJoin::Round),
            &mut BuffersBuilder::new(&mut buffers, |vertex: StrokeVertex| {
                let p = vertex.position();
                RgbVertexPx {
                    x: p.x,
                    y: p.y,
                    color,
                }
            }),
        )
        .ok()?;
    if buffers.indices.is_empty() {
        return None;
    }
    Some((buffers.vertices, buffers.indices))
}

fn circle_path(circle: CirclePath) -> Path {
    let mut builder = Path::builder();
    let radius = circle.radius.max(0.0);
    builder.begin(point(circle.center.x + radius, circle.center.y));
    for i in 1..UI3_CIRCLE_SEGMENTS {
        let t = (i as f32) * core::f32::consts::TAU / (UI3_CIRCLE_SEGMENTS as f32);
        builder.line_to(point(
            circle.center.x + radius * cos_f32(t),
            circle.center.y + radius * sin_f32(t),
        ));
    }
    builder.end(true);
    builder.build()
}

fn subpaths_to_path(subpaths: &[Vec<Ui3Point>], close: bool) -> Path {
    let mut builder = Path::builder();
    for subpath in subpaths {
        let Some(first) = subpath.first() else {
            continue;
        };
        builder.begin(point(first.x, first.y));
        for p in subpath.iter().skip(1) {
            builder.line_to(point(p.x, p.y));
        }
        builder.end(close);
    }
    builder.build()
}

fn push_line_to(path: &mut PendingPath, to: Ui3Point) {
    if path.subpaths.is_empty() {
        path.subpaths.push(Vec::from([Ui3Point::default()]));
    }
    if let Some(subpath) = path.subpaths.last_mut() {
        subpath.push(to);
    }
}

fn world_position(host: &Ui3PixiHost, node_id: Ui3NodeId) -> Ui3Point {
    let mut current = Some(node_id);
    let mut out = Ui3Point::default();
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = host.nodes().get(&id) else {
            break;
        };
        out.x += node.position.x;
        out.y += node.position.y;
        current = node.parent;
        depth += 1;
        if depth >= 128 {
            break;
        }
    }
    out
}

#[inline]
fn translate_point(point: Ui3Point, origin: Ui3Point) -> Ui3Point {
    Ui3Point {
        x: point.x + origin.x,
        y: point.y + origin.y,
    }
}

#[inline]
fn translate_rect(rect: Ui3Rect, origin: Ui3Point) -> Ui3Rect {
    Ui3Rect {
        x: rect.x + origin.x,
        y: rect.y + origin.y,
        ..rect
    }
}

#[inline]
fn color_to_rgba8(color: Ui3Color) -> Rgba8 {
    let mut rgba = argb_to_rgba8(color.rgba);
    rgba.scale_alpha(alpha_u8(color.alpha))
}

#[inline]
fn argb_to_rgba8(argb: u32) -> Rgba8 {
    Rgba8::new(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

#[inline]
fn alpha_u8(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}
