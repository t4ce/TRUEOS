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
    metric: SvgGlyphMetric,
    mesh: ImbaFontIconMesh,
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

macro_rules! svg_icon_asset {
    ($ch:literal, $path:literal) => {
        SvgIconAsset {
            ch: $ch,
            bytes: include_bytes!($path),
        }
    };
}

static IMBAFONT_ASSETS: &[SvgIconAsset] = &[
    svg_icon_asset!('0', "imbafont/imbasvgs/0.svg"),
    svg_icon_asset!('1', "imbafont/imbasvgs/1.svg"),
    svg_icon_asset!('2', "imbafont/imbasvgs/2.svg"),
    svg_icon_asset!('3', "imbafont/imbasvgs/3.svg"),
    svg_icon_asset!('4', "imbafont/imbasvgs/4.svg"),
    svg_icon_asset!('5', "imbafont/imbasvgs/5.svg"),
    svg_icon_asset!('6', "imbafont/imbasvgs/6.svg"),
    svg_icon_asset!('7', "imbafont/imbasvgs/7.svg"),
    svg_icon_asset!('8', "imbafont/imbasvgs/8.svg"),
    svg_icon_asset!('9', "imbafont/imbasvgs/9.svg"),
    svg_icon_asset!('a', "imbafont/imbasvgs/a.svg"),
    svg_icon_asset!('b', "imbafont/imbasvgs/b.svg"),
    svg_icon_asset!('c', "imbafont/imbasvgs/c.svg"),
    svg_icon_asset!('d', "imbafont/imbasvgs/d.svg"),
    svg_icon_asset!('e', "imbafont/imbasvgs/e.svg"),
    svg_icon_asset!('f', "imbafont/imbasvgs/f.svg"),
    svg_icon_asset!('g', "imbafont/imbasvgs/g.svg"),
    svg_icon_asset!('h', "imbafont/imbasvgs/h.svg"),
    svg_icon_asset!('i', "imbafont/imbasvgs/i.svg"),
    svg_icon_asset!('j', "imbafont/imbasvgs/j.svg"),
    svg_icon_asset!('k', "imbafont/imbasvgs/k.svg"),
    svg_icon_asset!('l', "imbafont/imbasvgs/l.svg"),
    svg_icon_asset!('m', "imbafont/imbasvgs/m.svg"),
    svg_icon_asset!('n', "imbafont/imbasvgs/n.svg"),
    svg_icon_asset!('o', "imbafont/imbasvgs/o.svg"),
    svg_icon_asset!('p', "imbafont/imbasvgs/p.svg"),
    svg_icon_asset!('q', "imbafont/imbasvgs/q.svg"),
    svg_icon_asset!('r', "imbafont/imbasvgs/r.svg"),
    svg_icon_asset!('s', "imbafont/imbasvgs/s.svg"),
    svg_icon_asset!('t', "imbafont/imbasvgs/t.svg"),
    svg_icon_asset!('u', "imbafont/imbasvgs/u.svg"),
    svg_icon_asset!('v', "imbafont/imbasvgs/v.svg"),
    svg_icon_asset!('w', "imbafont/imbasvgs/w.svg"),
    svg_icon_asset!('x', "imbafont/imbasvgs/x.svg"),
    svg_icon_asset!('y', "imbafont/imbasvgs/y.svg"),
    svg_icon_asset!('z', "imbafont/imbasvgs/z.svg"),
    svg_icon_asset!('A', "imbafont/imbasvgs/aa.svg"),
    svg_icon_asset!('B', "imbafont/imbasvgs/bb.svg"),
    svg_icon_asset!('C', "imbafont/imbasvgs/cc.svg"),
    svg_icon_asset!('D', "imbafont/imbasvgs/dd.svg"),
    svg_icon_asset!('E', "imbafont/imbasvgs/ee.svg"),
    svg_icon_asset!('F', "imbafont/imbasvgs/ff.svg"),
    svg_icon_asset!('G', "imbafont/imbasvgs/gg.svg"),
    svg_icon_asset!('H', "imbafont/imbasvgs/hh.svg"),
    svg_icon_asset!('I', "imbafont/imbasvgs/ii.svg"),
    svg_icon_asset!('J', "imbafont/imbasvgs/jj.svg"),
    svg_icon_asset!('K', "imbafont/imbasvgs/kk.svg"),
    svg_icon_asset!('L', "imbafont/imbasvgs/ll.svg"),
    svg_icon_asset!('M', "imbafont/imbasvgs/mm.svg"),
    svg_icon_asset!('N', "imbafont/imbasvgs/nn.svg"),
    svg_icon_asset!('O', "imbafont/imbasvgs/oo.svg"),
    svg_icon_asset!('P', "imbafont/imbasvgs/pp.svg"),
    svg_icon_asset!('Q', "imbafont/imbasvgs/qq.svg"),
    svg_icon_asset!('R', "imbafont/imbasvgs/rr.svg"),
    svg_icon_asset!('S', "imbafont/imbasvgs/ss.svg"),
    svg_icon_asset!('T', "imbafont/imbasvgs/tt.svg"),
    svg_icon_asset!('U', "imbafont/imbasvgs/uu.svg"),
    svg_icon_asset!('V', "imbafont/imbasvgs/vv.svg"),
    svg_icon_asset!('W', "imbafont/imbasvgs/ww.svg"),
    svg_icon_asset!('X', "imbafont/imbasvgs/xx.svg"),
    svg_icon_asset!('Y', "imbafont/imbasvgs/yy.svg"),
    svg_icon_asset!('Z', "imbafont/imbasvgs/zz.svg"),
];

static IMBAFONT_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();

fn imbafont_metrics() -> [Option<SvgGlyphMetric>; 128] {
    let mut metrics = [None; 128];
    let Ok(text) = core::str::from_utf8(include_bytes!("imbafont/imbasvgs/metrics.txt")) else {
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
        let mut ch_iter = ch_text.chars();
        let Some(ch) = ch_iter.next() else {
            continue;
        };
        if ch_iter.next().is_some() || (ch as usize) >= metrics.len() {
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

fn imbafont_icons() -> &'static Vec<ImbaFontIcon> {
    IMBAFONT_ICONS.call_once(|| {
        let metrics = imbafont_metrics();
        let mut icons = Vec::with_capacity(IMBAFONT_ASSETS.len());
        for asset in IMBAFONT_ASSETS {
            let Some(metric) = metrics.get(asset.ch as usize).copied().flatten() else {
                continue;
            };
            let Some(mesh) = build_svg_mesh(asset.bytes) else {
                continue;
            };
            icons.push(ImbaFontIcon { metric, mesh });
        }
        icons
    })
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

#[inline]
fn icon_scale(index: usize, total: usize, scale_start: f32, scale_end: f32) -> f32 {
    if total <= 1 {
        return 1.0;
    }

    let t = index as f32 / (total.saturating_sub(1)) as f32;
    let eased = t * t * (3.0 - 2.0 * t);
    scale_start + (scale_end - scale_start) * eased
}

pub fn layout_run_centered(
    view_w: f32,
    top_y: f32,
    tile_h: f32,
    scale_start: f32,
    scale_end: f32,
) -> Option<ImbaFontRunLayout> {
    let icons = imbafont_icons();
    if icons.is_empty() {
        return None;
    }

    let mut pen_x = 0.0f32;
    let mut vis_left = f32::INFINITY;
    let mut vis_right = f32::NEG_INFINITY;
    let mut vis_top = f32::INFINITY;
    let mut vis_bottom = f32::NEG_INFINITY;
    let icon_count = icons.len();

    for (index, icon) in icons.iter().enumerate() {
        let icon_tile_h = tile_h * icon_scale(index, icon_count, scale_start, scale_end);
        let metric = icon.metric;
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

pub fn draw_run_in_frame(
    layout: &ImbaFontRunLayout,
    view_w: u32,
    view_h: u32,
    rgb: (u8, u8, u8),
    alpha: u8,
    scale_start: f32,
    scale_end: f32,
) -> bool {
    let icons = imbafont_icons();
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