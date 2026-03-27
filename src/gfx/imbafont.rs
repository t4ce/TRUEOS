use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use lyon_geom::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule as LyonFillRule, FillTessellator, FillVertex,
    VertexBuffers,
};
use spin::Once;
use tiny_skia_path::{
    Path as TinyPath, PathBuilder as TinyPathBuilder, PathSegment, Point as TinyPoint,
    Transform as TinyTransform,
};
use usvg::{Node, Options, Tree};

#[derive(Clone, Copy)]
struct SvgGlyphMetric {
    baseline_y: f32,
    ink_top: f32,
    ink_bottom: f32,
    ink_left: f32,
    ink_right: f32,
    advance_w: f32,
}

struct SvgIconAsset {
    ch: char,
    bytes: &'static [u8],
}

struct ImbaFontIconMesh {
    width: f32,
    height: f32,
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

struct ImbaFontIcon {
    ch: char,
    metric: SvgGlyphMetric,
    mesh: ImbaFontIconMesh,
}

#[derive(Clone, Copy)]
struct ImbaFontLayoutMetric {
    metric: SvgGlyphMetric,
}

include!(concat!(env!("OUT_DIR"), "/generated_imbafont_fonts.rs"));

#[derive(Clone, Copy)]
pub struct ImbaFontRunLayout {
    pub origin_x: f32,
    pub baseline_y: f32,
    pub vis_left: f32,
    pub vis_right: f32,
    pub vis_top: f32,
    pub vis_bottom: f32,
    tile_h: f32,
}

pub struct ImbaFontMaskTexture {
    pub width: u32,
    pub height: u32,
    pub draw_x: f32,
    pub draw_y: f32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct ImbaFontGlyphMetricsPx {
    pub baseline: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub advance: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImbaFontRowBoxMode {
    InkBounds,
    FaceBounds,
}

#[derive(Clone, Copy, Debug)]
pub struct ImbaFontFaceRowMetricsPx {
    pub baseline: f32,
    pub row_top: f32,
    pub row_bottom: f32,
    pub ascent: f32,
    pub descent: f32,
    pub row_h: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ImbaFontTextRowMetricsPx {
    pub baseline: f32,
    pub row_top: f32,
    pub row_bottom: f32,
    pub ascent: f32,
    pub descent: f32,
    pub row_h: f32,
    pub ink_left: f32,
    pub ink_right: f32,
    pub ink_top: f32,
    pub ink_bottom: f32,
    pub advance_w: f32,
}

#[derive(Clone, Copy)]
struct ImbaFontInkRowMetricsPx {
    ink_left: f32,
    ink_right: f32,
    ink_top: f32,
    ink_bottom: f32,
    advance_w: f32,
    has_ink: bool,
}

#[inline]
fn raster_edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn rasterize_mask_triangle(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
) {
    let area = raster_edge(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
    if area.abs() <= 1e-6 {
        return;
    }

    let mut min_x = libm::floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
    let mut max_x = libm::ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
    let mut min_y = libm::floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
    let mut max_y = libm::ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;

    if min_x >= max_x || min_y >= max_y {
        return;
    }

    min_x = min_x.max(0);
    min_y = min_y.max(0);
    max_x = max_x.min(width as i32);
    max_y = max_y.min(height as i32);

    const SAMPLE_POINTS: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];

    for py in min_y..max_y {
        for px in min_x..max_x {
            let mut covered = 0u32;
            for (sx, sy) in SAMPLE_POINTS {
                let fx = px as f32 + sx;
                let fy = py as f32 + sy;
                let w0 = raster_edge(p1.0, p1.1, p2.0, p2.1, fx, fy);
                let w1 = raster_edge(p2.0, p2.1, p0.0, p0.1, fx, fy);
                let w2 = raster_edge(p0.0, p0.1, p1.0, p1.1, fx, fy);
                if (area > 0.0 && w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                    || (area < 0.0 && w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
                {
                    covered = covered.saturating_add(1);
                }
            }

            if covered == 0 {
                continue;
            }

            let idx = (py as usize)
                .saturating_mul(width as usize)
                .saturating_add(px as usize)
                .saturating_mul(4);
            if idx + 4 > rgba.len() {
                continue;
            }

            let alpha = (((covered * 255) + 2) / 4) as u8;
            rgba[idx] = 0xFF;
            rgba[idx + 1] = 0xFF;
            rgba[idx + 2] = 0xFF;
            rgba[idx + 3] = rgba[idx + 3].max(alpha);
        }
    }
}

#[inline]
fn space_advance(tile_h: f32) -> f32 {
    tile_h * 0.42
}

fn layout_metric_for_char(face: ImbaFontFace, ch: char) -> Option<SvgGlyphMetric> {
    let layout_metrics = layout_metrics_for_face(face);
    let assets = assets_for_face(face);

    for (index, asset) in assets.iter().enumerate() {
        if asset.ch == ch {
            return layout_metrics.get(index).map(|entry| entry.metric);
        }
    }

    None
}

fn icon_for_char(face: ImbaFontFace, ch: char) -> Option<&'static ImbaFontIcon> {
    let icons = icons_for_face(face);
    for icon in icons {
        if icon.ch == ch {
            return Some(icon);
        }
    }

    None
}

pub fn glyph_metrics_px(
    face: ImbaFontFace,
    ch: char,
    tile_h: f32,
) -> Option<ImbaFontGlyphMetricsPx> {
    if ch == ' ' {
        return Some(ImbaFontGlyphMetricsPx {
            baseline: 0.0,
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
            advance: space_advance(tile_h),
        });
    }

    let metric = layout_metric_for_char(face, ch)?;
    Some(ImbaFontGlyphMetricsPx {
        baseline: metric.baseline_y * tile_h,
        top: metric.ink_top * tile_h,
        bottom: metric.ink_bottom * tile_h,
        left: metric.ink_left * tile_h,
        right: metric.ink_right * tile_h,
        advance: metric.advance_w * tile_h,
    })
}

pub fn face_row_metrics_px(face: ImbaFontFace, tile_h: f32) -> Option<ImbaFontFaceRowMetricsPx> {
    if !tile_h.is_finite() || tile_h <= 0.0 {
        return None;
    }

    let mut baseline = 0.0f32;
    let mut row_top = f32::INFINITY;
    let mut row_bottom = f32::NEG_INFINITY;
    let mut found = false;

    for metric in layout_metrics_for_face(face) {
        let baseline_px = metric.metric.baseline_y * tile_h;
        baseline = baseline.max(baseline_px);
        row_top = row_top.min((metric.metric.ink_top - metric.metric.baseline_y) * tile_h);
        row_bottom = row_bottom.max((metric.metric.ink_bottom - metric.metric.baseline_y) * tile_h);
        found = true;
    }

    if !found || !row_top.is_finite() || !row_bottom.is_finite() {
        return None;
    }

    let ascent = (-row_top).max(0.0);
    let descent = row_bottom.max(0.0);
    Some(ImbaFontFaceRowMetricsPx {
        baseline,
        row_top,
        row_bottom,
        ascent,
        descent,
        row_h: (row_bottom - row_top).max(0.0),
    })
}

fn text_ink_row_metrics_px_tracked(
    face: ImbaFontFace,
    text: &[u8],
    tile_h: f32,
    tracking_px: f32,
) -> Option<ImbaFontInkRowMetricsPx> {
    if text.is_empty() || !tile_h.is_finite() || tile_h <= 0.0 {
        return None;
    }

    let mut pen_x = 0.0f32;
    let mut ink_left = f32::INFINITY;
    let mut ink_right = f32::NEG_INFINITY;
    let mut ink_top = f32::INFINITY;
    let mut ink_bottom = f32::NEG_INFINITY;
    let mut has_advance = false;
    let mut has_ink = false;

    for &byte in text {
        let ch = byte as char;
        if ch == '\n' {
            break;
        }
        if ch == ' ' {
            if has_advance {
                pen_x += tracking_px;
            }
            pen_x += space_advance(tile_h);
            has_advance = true;
            continue;
        }

        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        if has_advance {
            pen_x += tracking_px;
        }
        ink_left = ink_left.min(pen_x + metric.ink_left * tile_h);
        ink_right = ink_right.max(pen_x + metric.ink_right * tile_h);
        ink_top = ink_top.min((metric.ink_top - metric.baseline_y) * tile_h);
        ink_bottom = ink_bottom.max((metric.ink_bottom - metric.baseline_y) * tile_h);
        pen_x += metric.advance_w * tile_h;
        has_advance = true;
        has_ink = true;
    }

    if !has_advance {
        return None;
    }

    Some(ImbaFontInkRowMetricsPx {
        ink_left: if ink_left.is_finite() { ink_left } else { 0.0 },
        ink_right: if ink_right.is_finite() {
            ink_right
        } else {
            pen_x.max(0.0)
        },
        ink_top: if ink_top.is_finite() { ink_top } else { 0.0 },
        ink_bottom: if ink_bottom.is_finite() {
            ink_bottom
        } else {
            0.0
        },
        advance_w: pen_x.max(0.0),
        has_ink,
    })
}

pub fn text_row_metrics_px(
    face: ImbaFontFace,
    text: &[u8],
    tile_h: f32,
    row_box_mode: ImbaFontRowBoxMode,
) -> Option<ImbaFontTextRowMetricsPx> {
    text_row_metrics_px_tracked(face, text, tile_h, 0.0, row_box_mode)
}

pub fn text_row_metrics_px_tracked(
    face: ImbaFontFace,
    text: &[u8],
    tile_h: f32,
    tracking_px: f32,
    row_box_mode: ImbaFontRowBoxMode,
) -> Option<ImbaFontTextRowMetricsPx> {
    let ink = text_ink_row_metrics_px_tracked(face, text, tile_h, tracking_px)?;
    let (row_top, row_bottom) = match row_box_mode {
        ImbaFontRowBoxMode::InkBounds => {
            let fallback_bottom = if ink.has_ink {
                ink.ink_bottom
            } else {
                tile_h.max(0.0)
            };
            (ink.ink_top, fallback_bottom)
        }
        ImbaFontRowBoxMode::FaceBounds => face_row_metrics_px(face, tile_h)
            .map(|metrics| (metrics.row_top, metrics.row_bottom))
            .unwrap_or_else(|| {
                let fallback_bottom = if ink.has_ink {
                    ink.ink_bottom
                } else {
                    tile_h.max(0.0)
                };
                (ink.ink_top, fallback_bottom)
            }),
    };
    let ascent = (-row_top).max(0.0);
    let descent = row_bottom.max(0.0);

    Some(ImbaFontTextRowMetricsPx {
        baseline: -row_top,
        row_top,
        row_bottom,
        ascent,
        descent,
        row_h: (row_bottom - row_top).max(0.0),
        ink_left: ink.ink_left,
        ink_right: ink.ink_right,
        ink_top: ink.ink_top,
        ink_bottom: ink.ink_bottom,
        advance_w: ink.advance_w,
    })
}

pub fn rasterize_glyph_mask_texture(
    face: ImbaFontFace,
    ch: char,
    tile_h: f32,
    padding_px: f32,
) -> Option<ImbaFontMaskTexture> {
    if ch == ' ' {
        return None;
    }

    let metric = layout_metric_for_char(face, ch)?;
    let icon = icon_for_char(face, ch)?;
    let visible_w = ((metric.ink_right - metric.ink_left) * tile_h).max(1.0);
    let visible_h = ((metric.ink_bottom - metric.ink_top) * tile_h).max(1.0);
    let pad = padding_px.max(0.0);
    let draw_x = metric.ink_left * tile_h - pad;
    let draw_y = metric.ink_top * tile_h - pad;
    let width = libm::ceilf(visible_w + pad * 2.0).max(1.0) as u32;
    let height = libm::ceilf(visible_h + pad * 2.0).max(1.0) as u32;
    let mut rgba = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    let sx = tile_h / icon.mesh.width.max(0.0001);
    let sy = tile_h / icon.mesh.height.max(0.0001);
    let mut any_ink = false;

    for tri in icon.mesh.indices.chunks_exact(3) {
        let Some(v0) = icon.mesh.vertices.get(tri[0] as usize) else {
            continue;
        };
        let Some(v1) = icon.mesh.vertices.get(tri[1] as usize) else {
            continue;
        };
        let Some(v2) = icon.mesh.vertices.get(tri[2] as usize) else {
            continue;
        };

        let p0 = (v0[0] * sx - draw_x, v0[1] * sy - draw_y);
        let p1 = (v1[0] * sx - draw_x, v1[1] * sy - draw_y);
        let p2 = (v2[0] * sx - draw_x, v2[1] * sy - draw_y);
        rasterize_mask_triangle(&mut rgba, width, height, p0, p1, p2);
        any_ink = true;
    }

    if !any_ink {
        return None;
    }

    Some(ImbaFontMaskTexture {
        width,
        height,
        draw_x,
        draw_y,
        rgba,
    })
}

#[inline]
fn icon_scale(index: usize, total: usize, scale_start: f32, scale_end: f32) -> f32 {
    if total <= 1 {
        return 1.0;
    }

    let t = index as f32 / (total.saturating_sub(1)) as f32;
    let eased = t * t * (3.0 - 2.0 * t);
    scale_start + (scale_end - scale_start) * eased
}

fn parse_metrics(bytes: &[u8]) -> BTreeMap<char, SvgGlyphMetric> {
    let mut metrics = BTreeMap::new();
    let Ok(text) = core::str::from_utf8(bytes) else {
        return metrics;
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }

        let mut cols = trimmed.split_whitespace();
        let Some(ch_text) = cols.next() else {
            continue;
        };
        let mut chars = ch_text.chars();
        let Some(ch) = chars.next() else {
            continue;
        };
        if chars.next().is_some() {
            continue;
        }

        let Some(baseline_y) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let Some(ink_top) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let Some(ink_bottom) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let Some(ink_left) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let Some(ink_right) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };
        let _ = cols.next();
        let Some(advance_w) = cols.next().and_then(|v| v.parse::<f32>().ok()) else {
            continue;
        };

        metrics.insert(ch, SvgGlyphMetric {
            baseline_y,
            ink_top,
            ink_bottom,
            ink_left,
            ink_right,
            advance_w,
        });
    }

    metrics
}

fn build_svg_mesh(bytes: &[u8]) -> Option<ImbaFontIconMesh> {
    let svg_text = core::str::from_utf8(bytes).ok()?;
    let tree = Tree::from_str(svg_text, &Options::default()).ok()?;
    let size = tree.size();
    let mut fill_tess = FillTessellator::new();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    append_svg_group_mesh(tree.root(), &mut fill_tess, &mut vertices, &mut indices);
    if vertices.is_empty() || indices.is_empty() {
        return None;
    }

    Some(ImbaFontIconMesh {
        width: size.width().max(1.0),
        height: size.height().max(1.0),
        vertices,
        indices,
    })
}

fn append_svg_group_mesh(
    group: &usvg::Group,
    fill_tess: &mut FillTessellator,
    vertices: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    for node in group.children() {
        match node {
            Node::Group(group) => append_svg_group_mesh(group, fill_tess, vertices, indices),
            Node::Path(path) => {
                if !path.is_visible() {
                    continue;
                }

                let Some(path_data) = transform_tiny_path(path.data(), path.abs_transform()) else {
                    continue;
                };

                if let Some(fill) = path.fill() {
                    let lyon_path = tiny_path_to_lyon(&path_data);
                    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
                    let mut options = FillOptions::default();
                    options.fill_rule = match fill.rule() {
                        usvg::FillRule::NonZero => LyonFillRule::NonZero,
                        usvg::FillRule::EvenOdd => LyonFillRule::EvenOdd,
                    };
                    if fill_tess
                        .tessellate_path(
                            &lyon_path,
                            &options,
                            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                                [v.position().x, v.position().y]
                            }),
                        )
                        .is_ok()
                    {
                        append_svg_mesh_buffers(vertices, indices, &buffers);
                    }
                }

                if let Some(stroke) = path.stroke() {
                    let stroke_style = stroke.to_tiny_skia();
                    let Some(stroked) = path_data.stroke(&stroke_style, 1.0) else {
                        continue;
                    };
                    let lyon_path = tiny_path_to_lyon(&stroked);
                    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
                    if fill_tess
                        .tessellate_path(
                            &lyon_path,
                            &FillOptions::default(),
                            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                                [v.position().x, v.position().y]
                            }),
                        )
                        .is_ok()
                    {
                        append_svg_mesh_buffers(vertices, indices, &buffers);
                    }
                }
            }
            Node::Text(_) | Node::Image(_) => {}
        }
    }
}

fn append_svg_mesh_buffers(
    vertices: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    buffers: &VertexBuffers<[f32; 2], u32>,
) {
    let base = vertices.len() as u32;
    vertices.extend_from_slice(buffers.vertices.as_slice());
    indices.extend(buffers.indices.iter().map(|idx| base + *idx));
}

fn transform_tiny_path(path: &TinyPath, ts: TinyTransform) -> Option<TinyPath> {
    let mut builder = TinyPathBuilder::new();

    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                let p = transform_point(p, ts);
                builder.move_to(p.x, p.y);
            }
            PathSegment::LineTo(p) => {
                let p = transform_point(p, ts);
                builder.line_to(p.x, p.y);
            }
            PathSegment::QuadTo(p1, p) => {
                let p1 = transform_point(p1, ts);
                let p = transform_point(p, ts);
                builder.quad_to(p1.x, p1.y, p.x, p.y);
            }
            PathSegment::CubicTo(p1, p2, p) => {
                let p1 = transform_point(p1, ts);
                let p2 = transform_point(p2, ts);
                let p = transform_point(p, ts);
                builder.cubic_to(p1.x, p1.y, p2.x, p2.y, p.x, p.y);
            }
            PathSegment::Close => builder.close(),
        }
    }

    builder.finish()
}

fn transform_point(p: TinyPoint, ts: TinyTransform) -> TinyPoint {
    TinyPoint::from_xy(
        ts.sx * p.x + ts.kx * p.y + ts.tx,
        ts.ky * p.x + ts.sy * p.y + ts.ty,
    )
}

fn tiny_path_to_lyon(path: &TinyPath) -> LyonPath {
    let mut builder = LyonPath::builder();
    let mut subpath_open = false;

    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if subpath_open {
                    builder.end(false);
                }
                builder.begin(point(p.x, p.y));
                subpath_open = true;
            }
            PathSegment::LineTo(p) => {
                if subpath_open {
                    builder.line_to(point(p.x, p.y));
                }
            }
            PathSegment::QuadTo(p1, p) => {
                if subpath_open {
                    builder.quadratic_bezier_to(point(p1.x, p1.y), point(p.x, p.y));
                }
            }
            PathSegment::CubicTo(p1, p2, p) => {
                if subpath_open {
                    builder.cubic_bezier_to(point(p1.x, p1.y), point(p2.x, p2.y), point(p.x, p.y));
                }
            }
            PathSegment::Close => {
                if subpath_open {
                    builder.end(true);
                    subpath_open = false;
                }
            }
        }
    }

    if subpath_open {
        builder.end(false);
    }

    builder.build()
}

pub fn layout_run_centered(
    face: ImbaFontFace,
    view_w: f32,
    top_y: f32,
    tile_h: f32,
    scale_start: f32,
    scale_end: f32,
) -> Option<ImbaFontRunLayout> {
    let layout_metrics = layout_metrics_for_face(face);
    if layout_metrics.is_empty() {
        return None;
    }

    let mut pen_x = 0.0f32;
    let mut vis_left = f32::INFINITY;
    let mut vis_right = f32::NEG_INFINITY;
    let mut vis_top = f32::INFINITY;
    let mut vis_bottom = f32::NEG_INFINITY;
    let icon_count = layout_metrics.len();

    for (index, layout_metric) in layout_metrics.iter().enumerate() {
        let icon_tile_h = tile_h * icon_scale(index, icon_count, scale_start, scale_end);
        let metric = layout_metric.metric;
        vis_left = vis_left.min(pen_x + metric.ink_left * icon_tile_h);
        vis_right = vis_right.max(pen_x + metric.ink_right * icon_tile_h);
        vis_top = vis_top.min((metric.ink_top - metric.baseline_y) * icon_tile_h);
        vis_bottom = vis_bottom.max((metric.ink_bottom - metric.baseline_y) * icon_tile_h);
        pen_x += metric.advance_w * icon_tile_h;
    }

    if !vis_left.is_finite() || !vis_right.is_finite() || vis_right <= vis_left {
        return None;
    }

    let visible_w = vis_right - vis_left;
    let origin_x = ((view_w - visible_w) * 0.5).max(0.0) - vis_left;
    let baseline_y = top_y - vis_top;

    Some(ImbaFontRunLayout {
        origin_x,
        baseline_y,
        vis_left,
        vis_right,
        vis_top,
        vis_bottom,
        tile_h,
    })
}

pub fn layout_text_centered(
    face: ImbaFontFace,
    text: &[u8],
    view_w: f32,
    view_h: f32,
    tile_h: f32,
) -> Option<ImbaFontRunLayout> {
    if text.is_empty() {
        return None;
    }

    let mut pen_x = 0.0f32;
    let mut vis_left = f32::INFINITY;
    let mut vis_right = f32::NEG_INFINITY;
    let mut vis_top = f32::INFINITY;
    let mut vis_bottom = f32::NEG_INFINITY;

    for &byte in text {
        let ch = byte as char;
        if ch == ' ' {
            pen_x += space_advance(tile_h);
            continue;
        }

        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        vis_left = vis_left.min(pen_x + metric.ink_left * tile_h);
        vis_right = vis_right.max(pen_x + metric.ink_right * tile_h);
        vis_top = vis_top.min((metric.ink_top - metric.baseline_y) * tile_h);
        vis_bottom = vis_bottom.max((metric.ink_bottom - metric.baseline_y) * tile_h);
        pen_x += metric.advance_w * tile_h;
    }

    if !vis_left.is_finite() || !vis_right.is_finite() || vis_right <= vis_left {
        return None;
    }

    let visible_w = vis_right - vis_left;
    let visible_h = vis_bottom - vis_top;
    let origin_x = ((view_w - visible_w) * 0.5).max(0.0) - vis_left;
    let baseline_y = ((view_h - visible_h) * 0.5).max(0.0) - vis_top;

    Some(ImbaFontRunLayout {
        origin_x,
        baseline_y,
        vis_left,
        vis_right,
        vis_top,
        vis_bottom,
        tile_h,
    })
}

pub fn measure_text_width_px(face: ImbaFontFace, text: &[u8], tile_h: f32) -> f32 {
    measure_text_width_px_tracked(face, text, tile_h, 0.0)
}

pub fn measure_text_width_px_tracked(
    face: ImbaFontFace,
    text: &[u8],
    tile_h: f32,
    tracking_px: f32,
) -> f32 {
    if text.is_empty() || !tile_h.is_finite() || tile_h <= 0.0 {
        return 0.0;
    }

    let mut pen_x = 0.0f32;
    let mut vis_left = f32::INFINITY;
    let mut vis_right = f32::NEG_INFINITY;
    let mut line_has_ink = false;
    let mut line_has_advance = false;
    let mut max_w = 0.0f32;
    for &byte in text {
        let ch = byte as char;
        if ch == '\n' {
            if line_has_ink && vis_left.is_finite() && vis_right.is_finite() {
                max_w = max_w.max((vis_right - vis_left).max(0.0));
            }
            pen_x = 0.0;
            vis_left = f32::INFINITY;
            vis_right = f32::NEG_INFINITY;
            line_has_ink = false;
            line_has_advance = false;
            continue;
        }
        if ch == ' ' {
            if line_has_advance {
                pen_x += tracking_px;
            }
            pen_x += space_advance(tile_h);
            line_has_advance = true;
            continue;
        }
        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        if line_has_advance {
            pen_x += tracking_px;
        }
        vis_left = vis_left.min(pen_x + metric.ink_left * tile_h);
        vis_right = vis_right.max(pen_x + metric.ink_right * tile_h);
        pen_x += metric.advance_w * tile_h;
        line_has_ink = true;
        line_has_advance = true;
    }

    if line_has_ink && vis_left.is_finite() && vis_right.is_finite() {
        max_w = max_w.max((vis_right - vis_left).max(0.0));
    }

    max_w
}

pub fn layout_text_top_left(
    face: ImbaFontFace,
    text: &[u8],
    x: f32,
    top_y: f32,
    tile_h: f32,
) -> Option<ImbaFontRunLayout> {
    layout_text_top_left_tracked(face, text, x, top_y, tile_h, 0.0)
}

pub fn layout_text_row_top_left(
    face: ImbaFontFace,
    text: &[u8],
    x: f32,
    row_top_y: f32,
    tile_h: f32,
    row_box_mode: ImbaFontRowBoxMode,
) -> Option<ImbaFontRunLayout> {
    layout_text_row_top_left_tracked(face, text, x, row_top_y, tile_h, 0.0, row_box_mode)
}

pub fn layout_text_row_top_left_tracked(
    face: ImbaFontFace,
    text: &[u8],
    x: f32,
    row_top_y: f32,
    tile_h: f32,
    tracking_px: f32,
    row_box_mode: ImbaFontRowBoxMode,
) -> Option<ImbaFontRunLayout> {
    let metrics = text_row_metrics_px_tracked(face, text, tile_h, tracking_px, row_box_mode)?;

    Some(ImbaFontRunLayout {
        origin_x: x - metrics.ink_left,
        baseline_y: row_top_y - metrics.row_top,
        vis_left: metrics.ink_left,
        vis_right: metrics.ink_right,
        vis_top: metrics.ink_top,
        vis_bottom: metrics.ink_bottom,
        tile_h,
    })
}

pub fn layout_text_top_left_tracked(
    face: ImbaFontFace,
    text: &[u8],
    x: f32,
    top_y: f32,
    tile_h: f32,
    tracking_px: f32,
) -> Option<ImbaFontRunLayout> {
    layout_text_row_top_left_tracked(
        face,
        text,
        x,
        top_y,
        tile_h,
        tracking_px,
        ImbaFontRowBoxMode::InkBounds,
    )
}

pub fn rasterize_text_mask_texture(
    face: ImbaFontFace,
    text: &[u8],
    layout: &ImbaFontRunLayout,
    padding_px: f32,
) -> Option<ImbaFontMaskTexture> {
    if text.is_empty() {
        return None;
    }

    let visible_w = (layout.vis_right - layout.vis_left).max(1.0);
    let visible_h = (layout.vis_bottom - layout.vis_top).max(1.0);
    let pad = padding_px.max(0.0);
    let draw_x = layout.origin_x + layout.vis_left - pad;
    let draw_y = layout.baseline_y + layout.vis_top - pad;
    let width = libm::ceilf(visible_w + pad * 2.0).max(1.0) as u32;
    let height = libm::ceilf(visible_h + pad * 2.0).max(1.0) as u32;
    let mut rgba = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    let mut pen_x = layout.origin_x;
    let mut any_ink = false;

    for &byte in text {
        let ch = byte as char;
        if ch == ' ' {
            pen_x += space_advance(layout.tile_h);
            continue;
        }

        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        let Some(icon) = icon_for_char(face, ch) else {
            pen_x += metric.advance_w * layout.tile_h;
            continue;
        };

        let y = layout.baseline_y - metric.baseline_y * layout.tile_h;
        let sx = layout.tile_h / icon.mesh.width.max(0.0001);
        let sy = layout.tile_h / icon.mesh.height.max(0.0001);

        for tri in icon.mesh.indices.chunks_exact(3) {
            let Some(v0) = icon.mesh.vertices.get(tri[0] as usize) else {
                continue;
            };
            let Some(v1) = icon.mesh.vertices.get(tri[1] as usize) else {
                continue;
            };
            let Some(v2) = icon.mesh.vertices.get(tri[2] as usize) else {
                continue;
            };

            let p0 = (pen_x + v0[0] * sx - draw_x, y + v0[1] * sy - draw_y);
            let p1 = (pen_x + v1[0] * sx - draw_x, y + v1[1] * sy - draw_y);
            let p2 = (pen_x + v2[0] * sx - draw_x, y + v2[1] * sy - draw_y);
            rasterize_mask_triangle(&mut rgba, width, height, p0, p1, p2);
            any_ink = true;
        }

        pen_x += metric.advance_w * layout.tile_h;
    }

    if !any_ink {
        return None;
    }

    Some(ImbaFontMaskTexture {
        width,
        height,
        draw_x,
        draw_y,
        rgba,
    })
}

pub fn draw_run_in_frame(
    face: ImbaFontFace,
    layout: &ImbaFontRunLayout,
    view_w: u32,
    view_h: u32,
    rgb: (u8, u8, u8),
    alpha: u8,
    scale_start: f32,
    scale_end: f32,
) -> bool {
    let icons = icons_for_face(face);
    if icons.is_empty() {
        return false;
    }

    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    let icon_count = icons.len();
    let mut pen_x = layout.origin_x;

    for (index, icon) in icons.iter().enumerate() {
        let icon_tile_h = layout.tile_h * icon_scale(index, icon_count, scale_start, scale_end);
        let y = layout.baseline_y - icon.metric.baseline_y * icon_tile_h;
        let sx = icon_tile_h / icon.mesh.width.max(0.0001);
        let sy = icon_tile_h / icon.mesh.height.max(0.0001);
        let mut blob = Vec::with_capacity(icon.mesh.indices.len().saturating_mul(12));

        for &idx in &icon.mesh.indices {
            let Some(vertex) = icon.mesh.vertices.get(idx as usize) else {
                continue;
            };
            let px = pen_x + vertex[0] * sx;
            let py = y + vertex[1] * sy;
            let nx = (2.0 * (px / fb_w)) - 1.0;
            let ny = 1.0 - (2.0 * (py / fb_h));
            blob.extend_from_slice(&nx.to_le_bytes());
            blob.extend_from_slice(&ny.to_le_bytes());
            blob.push(rgb.0);
            blob.push(rgb.1);
            blob.push(rgb.2);
            blob.push(alpha);
        }

        if !blob.is_empty() {
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_blend(
                    1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0,
                )
            };
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
                    blob.as_ptr(),
                    blob.len(),
                )
            };
        }

        pen_x += icon.metric.advance_w * icon_tile_h;
    }

    true
}

pub fn draw_text_in_frame(
    face: ImbaFontFace,
    text: &[u8],
    layout: &ImbaFontRunLayout,
    view_w: u32,
    view_h: u32,
    rgb: (u8, u8, u8),
    alpha: u8,
) -> bool {
    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    let mut pen_x = layout.origin_x;

    for &byte in text {
        let ch = byte as char;
        if ch == ' ' {
            pen_x += space_advance(layout.tile_h);
            continue;
        }

        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        let Some(icon) = icon_for_char(face, ch) else {
            pen_x += metric.advance_w * layout.tile_h;
            continue;
        };

        let y = layout.baseline_y - metric.baseline_y * layout.tile_h;
        let sx = layout.tile_h / icon.mesh.width.max(0.0001);
        let sy = layout.tile_h / icon.mesh.height.max(0.0001);
        let mut blob = Vec::with_capacity(icon.mesh.indices.len().saturating_mul(12));

        for &idx in &icon.mesh.indices {
            let Some(vertex) = icon.mesh.vertices.get(idx as usize) else {
                continue;
            };
            let px = pen_x + vertex[0] * sx;
            let py = y + vertex[1] * sy;
            let nx = (2.0 * (px / fb_w)) - 1.0;
            let ny = 1.0 - (2.0 * (py / fb_h));
            blob.extend_from_slice(&nx.to_le_bytes());
            blob.extend_from_slice(&ny.to_le_bytes());
            blob.push(rgb.0);
            blob.push(rgb.1);
            blob.push(rgb.2);
            blob.push(alpha);
        }

        if !blob.is_empty() {
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_blend(
                    1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0,
                )
            };
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
                    blob.as_ptr(),
                    blob.len(),
                )
            };
        }

        pen_x += metric.advance_w * layout.tile_h;
    }

    true
}

pub fn draw_text_in_frame_negative(
    face: ImbaFontFace,
    text: &[u8],
    layout: &ImbaFontRunLayout,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    let mut pen_x = layout.origin_x;

    for &byte in text {
        let ch = byte as char;
        if ch == ' ' {
            pen_x += space_advance(layout.tile_h);
            continue;
        }

        let Some(metric) = layout_metric_for_char(face, ch) else {
            continue;
        };
        let Some(icon) = icon_for_char(face, ch) else {
            pen_x += metric.advance_w * layout.tile_h;
            continue;
        };

        let y = layout.baseline_y - metric.baseline_y * layout.tile_h;
        let sx = layout.tile_h / icon.mesh.width.max(0.0001);
        let sy = layout.tile_h / icon.mesh.height.max(0.0001);
        let mut blob = Vec::with_capacity(icon.mesh.indices.len().saturating_mul(12));

        for &idx in &icon.mesh.indices {
            let Some(vertex) = icon.mesh.vertices.get(idx as usize) else {
                continue;
            };
            let px = pen_x + vertex[0] * sx;
            let py = y + vertex[1] * sy;
            let nx = (2.0 * (px / fb_w)) - 1.0;
            let ny = 1.0 - (2.0 * (py / fb_h));
            blob.extend_from_slice(&nx.to_le_bytes());
            blob.extend_from_slice(&ny.to_le_bytes());
            blob.push(0xFF);
            blob.push(0xFF);
            blob.push(0xFF);
            blob.push(alpha);
        }

        if !blob.is_empty() {
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0307, 0, 0x0307, 0, 0, 0)
            };
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
                    blob.as_ptr(),
                    blob.len(),
                )
            };
            let _ = unsafe {
                crate::r::io::cabi::trueos_cabi_gfx_set_blend(
                    1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0,
                )
            };
        }

        pen_x += metric.advance_w * layout.tile_h;
    }

    true
}
