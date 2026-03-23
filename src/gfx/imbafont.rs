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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImbaFontFace {
    Regular,
    Block,
    Grow,
    Impact,
}

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

macro_rules! svg_icon_asset {
    ($ch:literal, $dir:literal, $file:literal) => {
        SvgIconAsset {
            ch: $ch,
            bytes: include_bytes!(concat!("imbafont/", $dir, "/", $file)),
        }
    };
}

macro_rules! define_font_assets {
    ($name:ident, $dir:literal) => {
        static $name: &[SvgIconAsset] = &[
            svg_icon_asset!('0', $dir, "0.svg"),
            svg_icon_asset!('1', $dir, "1.svg"),
            svg_icon_asset!('2', $dir, "2.svg"),
            svg_icon_asset!('3', $dir, "3.svg"),
            svg_icon_asset!('4', $dir, "4.svg"),
            svg_icon_asset!('5', $dir, "5.svg"),
            svg_icon_asset!('6', $dir, "6.svg"),
            svg_icon_asset!('7', $dir, "7.svg"),
            svg_icon_asset!('8', $dir, "8.svg"),
            svg_icon_asset!('9', $dir, "9.svg"),
            svg_icon_asset!('a', $dir, "a.svg"),
            svg_icon_asset!('b', $dir, "b.svg"),
            svg_icon_asset!('c', $dir, "c.svg"),
            svg_icon_asset!('d', $dir, "d.svg"),
            svg_icon_asset!('e', $dir, "e.svg"),
            svg_icon_asset!('f', $dir, "f.svg"),
            svg_icon_asset!('g', $dir, "g.svg"),
            svg_icon_asset!('h', $dir, "h.svg"),
            svg_icon_asset!('i', $dir, "i.svg"),
            svg_icon_asset!('j', $dir, "j.svg"),
            svg_icon_asset!('k', $dir, "k.svg"),
            svg_icon_asset!('l', $dir, "l.svg"),
            svg_icon_asset!('m', $dir, "m.svg"),
            svg_icon_asset!('n', $dir, "n.svg"),
            svg_icon_asset!('o', $dir, "o.svg"),
            svg_icon_asset!('p', $dir, "p.svg"),
            svg_icon_asset!('q', $dir, "q.svg"),
            svg_icon_asset!('r', $dir, "r.svg"),
            svg_icon_asset!('s', $dir, "s.svg"),
            svg_icon_asset!('t', $dir, "t.svg"),
            svg_icon_asset!('u', $dir, "u.svg"),
            svg_icon_asset!('v', $dir, "v.svg"),
            svg_icon_asset!('w', $dir, "w.svg"),
            svg_icon_asset!('x', $dir, "x.svg"),
            svg_icon_asset!('y', $dir, "y.svg"),
            svg_icon_asset!('z', $dir, "z.svg"),
            svg_icon_asset!('A', $dir, "aa.svg"),
            svg_icon_asset!('B', $dir, "bb.svg"),
            svg_icon_asset!('C', $dir, "cc.svg"),
            svg_icon_asset!('D', $dir, "dd.svg"),
            svg_icon_asset!('E', $dir, "ee.svg"),
            svg_icon_asset!('F', $dir, "ff.svg"),
            svg_icon_asset!('G', $dir, "gg.svg"),
            svg_icon_asset!('H', $dir, "hh.svg"),
            svg_icon_asset!('I', $dir, "ii.svg"),
            svg_icon_asset!('J', $dir, "jj.svg"),
            svg_icon_asset!('K', $dir, "kk.svg"),
            svg_icon_asset!('L', $dir, "ll.svg"),
            svg_icon_asset!('M', $dir, "mm.svg"),
            svg_icon_asset!('N', $dir, "nn.svg"),
            svg_icon_asset!('O', $dir, "oo.svg"),
            svg_icon_asset!('P', $dir, "pp.svg"),
            svg_icon_asset!('Q', $dir, "qq.svg"),
            svg_icon_asset!('R', $dir, "rr.svg"),
            svg_icon_asset!('S', $dir, "ss.svg"),
            svg_icon_asset!('T', $dir, "tt.svg"),
            svg_icon_asset!('U', $dir, "uu.svg"),
            svg_icon_asset!('V', $dir, "vv.svg"),
            svg_icon_asset!('W', $dir, "ww.svg"),
            svg_icon_asset!('X', $dir, "xx.svg"),
            svg_icon_asset!('Y', $dir, "yy.svg"),
            svg_icon_asset!('Z', $dir, "zz.svg"),
        ];
    };
}

define_font_assets!(IMBAFONT_REGULAR_ASSETS, "imbasvgs");
define_font_assets!(IMBAFONT_BLOCK_ASSETS, "imbasvgs_block");
define_font_assets!(IMBAFONT_GROW_ASSETS, "imbasvgs_grow");
define_font_assets!(IMBAFONT_IMPACT_ASSETS, "imbasvgs_impact");

static IMBAFONT_REGULAR_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();
static IMBAFONT_BLOCK_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();
static IMBAFONT_GROW_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();
static IMBAFONT_IMPACT_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();
static IMBAFONT_REGULAR_LAYOUT_METRICS: Once<Vec<ImbaFontLayoutMetric>> = Once::new();
static IMBAFONT_BLOCK_LAYOUT_METRICS: Once<Vec<ImbaFontLayoutMetric>> = Once::new();
static IMBAFONT_GROW_LAYOUT_METRICS: Once<Vec<ImbaFontLayoutMetric>> = Once::new();
static IMBAFONT_IMPACT_LAYOUT_METRICS: Once<Vec<ImbaFontLayoutMetric>> = Once::new();

#[inline]
fn icon_scale(index: usize, total: usize, scale_start: f32, scale_end: f32) -> f32 {
    if total <= 1 {
        return 1.0;
    }

    let t = index as f32 / (total.saturating_sub(1)) as f32;
    let eased = t * t * (3.0 - 2.0 * t);
    scale_start + (scale_end - scale_start) * eased
}

fn metrics_bytes_for_face(face: ImbaFontFace) -> &'static [u8] {
    match face {
        ImbaFontFace::Regular => include_bytes!("imbafont/imbasvgs/metrics.txt"),
        ImbaFontFace::Block => include_bytes!("imbafont/imbasvgs_block/metrics.txt"),
        ImbaFontFace::Grow => include_bytes!("imbafont/imbasvgs_grow/metrics.txt"),
        ImbaFontFace::Impact => include_bytes!("imbafont/imbasvgs_impact/metrics.txt"),
    }
}

fn assets_for_face(face: ImbaFontFace) -> &'static [SvgIconAsset] {
    match face {
        ImbaFontFace::Regular => IMBAFONT_REGULAR_ASSETS,
        ImbaFontFace::Block => IMBAFONT_BLOCK_ASSETS,
        ImbaFontFace::Grow => IMBAFONT_GROW_ASSETS,
        ImbaFontFace::Impact => IMBAFONT_IMPACT_ASSETS,
    }
}

fn parse_metrics(bytes: &[u8]) -> [Option<SvgGlyphMetric>; 128] {
    let mut metrics = [None; 128];
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
        if chars.next().is_some() || (ch as usize) >= metrics.len() {
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

        metrics[ch as usize] = Some(SvgGlyphMetric {
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

fn icons_for_face(face: ImbaFontFace) -> &'static Vec<ImbaFontIcon> {
    let metrics = parse_metrics(metrics_bytes_for_face(face));
    let assets = assets_for_face(face);

    let build = || {
        let mut icons = Vec::with_capacity(assets.len());
        for asset in assets {
            let Some(metric) = metrics.get(asset.ch as usize).copied().flatten() else {
                continue;
            };
            let Some(mesh) = build_svg_mesh(asset.bytes) else {
                continue;
            };
            icons.push(ImbaFontIcon {
                ch: asset.ch,
                metric,
                mesh,
            });
        }
        icons
    };

    match face {
        ImbaFontFace::Regular => IMBAFONT_REGULAR_ICONS.call_once(build),
        ImbaFontFace::Block => IMBAFONT_BLOCK_ICONS.call_once(build),
        ImbaFontFace::Grow => IMBAFONT_GROW_ICONS.call_once(build),
        ImbaFontFace::Impact => IMBAFONT_IMPACT_ICONS.call_once(build),
    }
}

fn layout_metrics_for_face(face: ImbaFontFace) -> &'static Vec<ImbaFontLayoutMetric> {
    let metrics = parse_metrics(metrics_bytes_for_face(face));
    let assets = assets_for_face(face);

    let build = || {
        let mut layout_metrics = Vec::with_capacity(assets.len());
        for asset in assets {
            let Some(metric) = metrics.get(asset.ch as usize).copied().flatten() else {
                continue;
            };
            layout_metrics.push(ImbaFontLayoutMetric { metric });
        }
        layout_metrics
    };

    match face {
        ImbaFontFace::Regular => IMBAFONT_REGULAR_LAYOUT_METRICS.call_once(build),
        ImbaFontFace::Block => IMBAFONT_BLOCK_LAYOUT_METRICS.call_once(build),
        ImbaFontFace::Grow => IMBAFONT_GROW_LAYOUT_METRICS.call_once(build),
        ImbaFontFace::Impact => IMBAFONT_IMPACT_LAYOUT_METRICS.call_once(build),
    }
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
