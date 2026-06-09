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
        clip: Option<Ui3Rect>,
    },
    Mesh {
        node: Ui3NodeId,
        kind: Ui3MeshKind,
        vertices: Vec<RgbVertexPx>,
        indices: Vec<u16>,
        clip: Option<Ui3Rect>,
    },
    TextureRect {
        node: Ui3NodeId,
        tex_id: u32,
        rect: Ui3Rect,
        alpha: f32,
        clip: Option<Ui3Rect>,
    },
    TextRun {
        node: Ui3NodeId,
        origin: Ui3Point,
        text: String,
        color: Rgba8,
        font_tier: u8,
        clip: Option<Ui3Rect>,
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

#[derive(Copy, Clone, Debug)]
struct EllipsePath {
    center: Ui3Point,
    rx: f32,
    ry: f32,
}

#[derive(Copy, Clone, Debug)]
struct RoundRectPath {
    rect: Ui3Rect,
    radius: f32,
}

#[derive(Clone, Debug, Default)]
struct PendingPath {
    rects: Vec<Ui3Rect>,
    round_rects: Vec<RoundRectPath>,
    circles: Vec<CirclePath>,
    ellipses: Vec<EllipsePath>,
    subpaths: Vec<Vec<Ui3Point>>,
}

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

impl PendingPath {
    fn is_empty(&self) -> bool {
        self.rects.is_empty()
            && self.round_rects.is_empty()
            && self.circles.is_empty()
            && self.ellipses.is_empty()
            && self.subpaths.is_empty()
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.round_rects.clear();
        self.circles.clear();
        self.ellipses.clear();
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
        let transform = world_transform(host, *node_id);
        let alpha = world_alpha(host, *node_id);
        let clip = world_clip(host, *node_id);
        match node.kind {
            Ui3NodeKind::Graphics => {
                lower_graphics_node(node, transform, alpha, clip, &mut out.draws)
            }
            Ui3NodeKind::Text if !node.text.is_empty() => out.draws.push(Ui3LoweredDraw::TextRun {
                node: node.id,
                origin: transform_point(Ui3Point::default(), transform),
                text: node.text.clone(),
                color: color_to_rgba8_with_alpha(node.text_fill, alpha),
                font_tier: node.text_font_tier,
                clip,
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
            Ui3LoweredDraw::SolidRect {
                rect, color, clip, ..
            } => {
                let Some(rect) = clipped_rect(*rect, *clip) else {
                    continue;
                };
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
            Ui3LoweredDraw::TextureRect { .. } => {}
            Ui3LoweredDraw::TextRun { .. } => {}
        }
    }
}

fn lower_graphics_node(
    node: &Ui3Node,
    transform: Ui3Transform,
    node_alpha: f32,
    clip: Option<Ui3Rect>,
    draws: &mut Vec<Ui3LoweredDraw>,
) {
    let mut pending = PendingPath::default();
    let mut last_painted = PendingPath::default();

    for op in &node.graphics {
        match *op {
            Ui3GraphicsOp::Rect(rect) => pending.rects.push(transform_rect(rect, transform)),
            Ui3GraphicsOp::RoundRect { rect, radius } => pending.round_rects.push(RoundRectPath {
                rect: transform_rect(rect, transform),
                radius: radius * stroke_scale(transform),
            }),
            Ui3GraphicsOp::Circle { center, radius } => {
                let rx = radius * transform.sx.abs();
                let ry = radius * transform.sy.abs();
                if nearly_equal(rx, ry) {
                    pending.circles.push(CirclePath {
                        center: transform_point(center, transform),
                        radius: rx,
                    });
                } else {
                    pending.ellipses.push(EllipsePath {
                        center: transform_point(center, transform),
                        rx,
                        ry,
                    });
                }
            }
            Ui3GraphicsOp::Ellipse { center, rx, ry } => pending.ellipses.push(EllipsePath {
                center: transform_point(center, transform),
                rx: rx * transform.sx.abs(),
                ry: ry * transform.sy.abs(),
            }),
            Ui3GraphicsOp::MoveTo(to) => pending
                .subpaths
                .push(Vec::from([transform_point(to, transform)])),
            Ui3GraphicsOp::LineTo(to) => push_line_to(&mut pending, transform_point(to, transform)),
            Ui3GraphicsOp::ClosePath => close_current_subpath(&mut pending),
            Ui3GraphicsOp::Fill(color) => {
                if !pending.is_empty() {
                    emit_fill(
                        node.id,
                        &pending,
                        color_to_rgba8_with_alpha(color, node_alpha),
                        clip,
                        draws,
                    );
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
                        color_to_rgba8_with_alpha(color, node_alpha),
                        width.max(UI3_DEFAULT_STROKE_WIDTH) * stroke_scale(transform),
                        clip,
                        draws,
                    );
                    if !pending.is_empty() {
                        last_painted = pending.clone();
                        pending.clear();
                    }
                }
            }
            Ui3GraphicsOp::TextureRect {
                tex_id,
                rect,
                alpha,
            } => {
                if !pending.is_empty() {
                    last_painted = pending.clone();
                    pending.clear();
                }
                draws.push(Ui3LoweredDraw::TextureRect {
                    node: node.id,
                    tex_id,
                    rect: transform_rect(rect, transform),
                    alpha: (alpha * node_alpha).clamp(0.0, 1.0),
                    clip,
                });
            }
        }
    }
}

fn emit_fill(
    node: Ui3NodeId,
    path: &PendingPath,
    color: Rgba8,
    clip: Option<Ui3Rect>,
    draws: &mut Vec<Ui3LoweredDraw>,
) {
    for rect in &path.rects {
        draws.push(Ui3LoweredDraw::SolidRect {
            node,
            rect: *rect,
            color,
            kind: Ui3SolidRectKind::Fill,
            clip,
        });
    }

    for round_rect in &path.round_rects {
        if let Some((vertices, indices)) =
            tessellate_fill_path(&round_rect_path(*round_rect), color)
        {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Fill,
                vertices,
                indices,
                clip,
            });
        }
    }

    for circle in &path.circles {
        if let Some((vertices, indices)) = tessellate_fill_path(&circle_path(*circle), color) {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Fill,
                vertices,
                indices,
                clip,
            });
        }
    }

    for ellipse in &path.ellipses {
        if let Some((vertices, indices)) = tessellate_fill_path(&ellipse_path(*ellipse), color) {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Fill,
                vertices,
                indices,
                clip,
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
            clip,
        });
    }
}

fn emit_stroke(
    node: Ui3NodeId,
    path: &PendingPath,
    color: Rgba8,
    width: f32,
    clip: Option<Ui3Rect>,
    draws: &mut Vec<Ui3LoweredDraw>,
) {
    for rect in &path.rects {
        emit_rect_stroke(node, *rect, color, width, clip, draws);
    }

    for round_rect in &path.round_rects {
        if let Some((vertices, indices)) =
            tessellate_stroke_path(&round_rect_path(*round_rect), color, width)
        {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Stroke,
                vertices,
                indices,
                clip,
            });
        }
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
                clip,
            });
        }
    }

    for ellipse in &path.ellipses {
        if let Some((vertices, indices)) =
            tessellate_stroke_path(&ellipse_path(*ellipse), color, width)
        {
            draws.push(Ui3LoweredDraw::Mesh {
                node,
                kind: Ui3MeshKind::Stroke,
                vertices,
                indices,
                clip,
            });
        }
    }

    let fallback_subpaths =
        emit_axis_aligned_stroke_subpaths(node, &path.subpaths, color, width, clip, draws);
    if let Some((vertices, indices)) =
        tessellate_stroke_path(&subpaths_to_path(&fallback_subpaths, false), color, width)
    {
        draws.push(Ui3LoweredDraw::Mesh {
            node,
            kind: Ui3MeshKind::Stroke,
            vertices,
            indices,
            clip,
        });
    }
}

fn emit_axis_aligned_stroke_subpaths(
    node: Ui3NodeId,
    subpaths: &[Vec<Ui3Point>],
    color: Rgba8,
    width: f32,
    clip: Option<Ui3Rect>,
    draws: &mut Vec<Ui3LoweredDraw>,
) -> Vec<Vec<Ui3Point>> {
    let mut fallback = Vec::new();
    for subpath in subpaths {
        if subpath.len() < 2 {
            continue;
        }
        if emit_axis_aligned_stroke_subpath(node, subpath, color, width, clip, draws) {
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
    clip: Option<Ui3Rect>,
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
            clip,
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
    clip: Option<Ui3Rect>,
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
            clip,
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

fn ellipse_path(ellipse: EllipsePath) -> Path {
    let mut builder = Path::builder();
    let rx = ellipse.rx.max(0.0);
    let ry = ellipse.ry.max(0.0);
    builder.begin(point(ellipse.center.x + rx, ellipse.center.y));
    for i in 1..UI3_CIRCLE_SEGMENTS {
        let t = (i as f32) * core::f32::consts::TAU / (UI3_CIRCLE_SEGMENTS as f32);
        builder
            .line_to(point(ellipse.center.x + rx * cos_f32(t), ellipse.center.y + ry * sin_f32(t)));
    }
    builder.end(true);
    builder.build()
}

fn round_rect_path(round_rect: RoundRectPath) -> Path {
    let rect = round_rect.rect;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return Path::builder().build();
    }
    let radius = round_rect
        .radius
        .max(0.0)
        .min(rect.w * 0.5)
        .min(rect.h * 0.5);
    if radius <= 0.0 {
        return rect_path(rect);
    }

    let mut builder = Path::builder();
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.w;
    let bottom = rect.y + rect.h;
    builder.begin(point(left + radius, top));
    builder.line_to(point(right - radius, top));
    append_arc(
        &mut builder,
        right - radius,
        top + radius,
        radius,
        -core::f32::consts::FRAC_PI_2,
        0.0,
    );
    builder.line_to(point(right, bottom - radius));
    append_arc(
        &mut builder,
        right - radius,
        bottom - radius,
        radius,
        0.0,
        core::f32::consts::FRAC_PI_2,
    );
    builder.line_to(point(left + radius, bottom));
    append_arc(
        &mut builder,
        left + radius,
        bottom - radius,
        radius,
        core::f32::consts::FRAC_PI_2,
        core::f32::consts::PI,
    );
    builder.line_to(point(left, top + radius));
    append_arc(
        &mut builder,
        left + radius,
        top + radius,
        radius,
        core::f32::consts::PI,
        core::f32::consts::PI + core::f32::consts::FRAC_PI_2,
    );
    builder.end(true);
    builder.build()
}

fn rect_path(rect: Ui3Rect) -> Path {
    let mut builder = Path::builder();
    if rect.w > 0.0 && rect.h > 0.0 {
        builder.begin(point(rect.x, rect.y));
        builder.line_to(point(rect.x + rect.w, rect.y));
        builder.line_to(point(rect.x + rect.w, rect.y + rect.h));
        builder.line_to(point(rect.x, rect.y + rect.h));
        builder.end(true);
    }
    builder.build()
}

fn append_arc(
    builder: &mut lyon_tessellation::path::Builder,
    cx: f32,
    cy: f32,
    radius: f32,
    start: f32,
    end: f32,
) {
    const STEPS: usize = 8;
    for i in 1..=STEPS {
        let t = start + (end - start) * (i as f32) / (STEPS as f32);
        builder.line_to(point(cx + radius * cos_f32(t), cy + radius * sin_f32(t)));
    }
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
        path.subpaths.push(Vec::from([to]));
        return;
    }
    if let Some(subpath) = path.subpaths.last_mut() {
        subpath.push(to);
    }
}

fn close_current_subpath(path: &mut PendingPath) {
    let Some(subpath) = path.subpaths.last_mut() else {
        return;
    };
    if subpath.len() < 2 {
        return;
    }
    let first = subpath[0];
    let last = *subpath.last().unwrap_or(&first);
    if !nearly_equal(first.x, last.x) || !nearly_equal(first.y, last.y) {
        subpath.push(first);
    }
}

fn world_clip(host: &Ui3PixiHost, node_id: Ui3NodeId) -> Option<Ui3Rect> {
    let mut current = Some(node_id);
    let mut clip = None;
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = host.nodes().get(&id) else {
            break;
        };
        if let Some(mask_id) = node.mask
            && let Some(mask_rect) = mask_world_bounds(host, mask_id)
        {
            clip = match clip {
                Some(existing) => intersect_rect(existing, mask_rect),
                None => Some(mask_rect),
            };
            clip?;
        }
        current = node.parent;
        depth += 1;
        if depth >= 128 {
            break;
        }
    }
    clip
}

fn mask_world_bounds(host: &Ui3PixiHost, mask_id: Ui3NodeId) -> Option<Ui3Rect> {
    let node = host.nodes().get(&mask_id)?;
    let local = graphics_local_bounds(&node.graphics)?;
    Some(transform_rect(local, world_transform(host, mask_id)))
}

fn graphics_local_bounds(ops: &[Ui3GraphicsOp]) -> Option<Ui3Rect> {
    let mut rect = None;
    let mut current = None;
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
            Ui3GraphicsOp::MoveTo(to) => {
                current = Some(to);
                rect = union_optional_point(rect, to);
            }
            Ui3GraphicsOp::LineTo(to) => {
                current = Some(to);
                rect = union_optional_point(rect, to);
            }
            Ui3GraphicsOp::ClosePath | Ui3GraphicsOp::Fill(_) | Ui3GraphicsOp::Stroke { .. } => {}
        }
    }
    let _ = current;
    rect
}

fn world_transform(host: &Ui3PixiHost, node_id: Ui3NodeId) -> Ui3Transform {
    let mut current = Some(node_id);
    let mut chain = Vec::new();
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = host.nodes().get(&id) else {
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
        let Some(node) = host.nodes().get(&id) else {
            continue;
        };
        out.tx += node.position.x * out.sx;
        out.ty += node.position.y * out.sy;
        out.sx *= sanitize_scale(node.scale.x);
        out.sy *= sanitize_scale(node.scale.y);
    }
    out
}

fn world_alpha(host: &Ui3PixiHost, node_id: Ui3NodeId) -> f32 {
    let mut current = Some(node_id);
    let mut out = 1.0f32;
    let mut depth = 0usize;
    while let Some(id) = current {
        let Some(node) = host.nodes().get(&id) else {
            break;
        };
        out *= node.alpha.clamp(0.0, 1.0);
        current = node.parent;
        depth += 1;
        if depth >= 128 {
            break;
        }
    }
    out.clamp(0.0, 1.0)
}

#[inline]
fn transform_point(point: Ui3Point, transform: Ui3Transform) -> Ui3Point {
    Ui3Point {
        x: transform.tx + point.x * transform.sx,
        y: transform.ty + point.y * transform.sy,
    }
}

#[inline]
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

#[inline]
fn stroke_scale(transform: Ui3Transform) -> f32 {
    transform.sx.abs().max(transform.sy.abs()).max(0.001)
}

#[inline]
fn sanitize_scale(value: f32) -> f32 {
    if value.is_finite() { value } else { 1.0 }
}

fn clipped_rect(rect: Ui3Rect, clip: Option<Ui3Rect>) -> Option<Ui3Rect> {
    match clip {
        Some(clip) => intersect_rect(rect, clip),
        None => Some(rect),
    }
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

fn union_optional_rect(current: Option<Ui3Rect>, next: Ui3Rect) -> Option<Ui3Rect> {
    if next.w <= 0.0 || next.h <= 0.0 {
        return current;
    }
    Some(match current {
        Some(current) => {
            let x0 = current.x.min(next.x);
            let y0 = current.y.min(next.y);
            let x1 = (current.x + current.w).max(next.x + next.w);
            let y1 = (current.y + current.h).max(next.y + next.h);
            Ui3Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            }
        }
        None => next,
    })
}

fn union_optional_point(current: Option<Ui3Rect>, point: Ui3Point) -> Option<Ui3Rect> {
    let point_rect = Ui3Rect {
        x: point.x,
        y: point.y,
        w: 0.01,
        h: 0.01,
    };
    union_optional_rect(current, point_rect)
}

#[inline]
fn color_to_rgba8(color: Ui3Color) -> Rgba8 {
    let mut rgba = argb_to_rgba8(color.rgba);
    rgba.scale_alpha(alpha_u8(color.alpha))
}

#[inline]
fn color_to_rgba8_with_alpha(color: Ui3Color, alpha: f32) -> Rgba8 {
    color_to_rgba8(Ui3Color {
        alpha: color.alpha * alpha,
        ..color
    })
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
