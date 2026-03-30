use alloc::vec;
use alloc::vec::Vec;
use core::str;

use libm::{ceilf, floorf, sqrtf};
use lyon_geom::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule as LyonFillRule, FillTessellator, FillVertex,
    VertexBuffers,
};
use tiny_skia_path::{
    Path as TinyPath, PathBuilder as TinyPathBuilder, PathSegment, Point as TinyPoint,
    Transform as TinyTransform,
};
use usvg::{Group, Node, Options, Paint, SpreadMethod, Stroke, Tree};

const SVG_MAX_TEXTURE_SIDE: u32 = 2048;
const ERR_SVG_INVALID_UTF8: i32 = -20;
const ERR_SVG_PARSE: i32 = -21;
const ERR_SVG_SIZE: i32 = -22;
const ERR_SVG_UPLOAD: i32 = -23;

#[derive(Clone, Copy, Debug)]
pub struct SvgTextureInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct SvgMeshInfo {
    pub width: u32,
    pub height: u32,
    pub svg_width: f32,
    pub svg_height: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SvgGradientStop {
    pub offset: f32,
    pub rgba: [f32; 4],
}

#[derive(Clone, Debug)]
pub enum SvgPaintStyle {
    Solid {
        rgba: [f32; 4],
    },
    Linear {
        p0: [f32; 2],
        p1: [f32; 2],
        spread: SpreadMethod,
        stops: Vec<SvgGradientStop>,
    },
    Radial {
        center: [f32; 2],
        radius: f32,
        spread: SpreadMethod,
        stops: Vec<SvgGradientStop>,
    },
}

#[derive(Clone, Debug)]
pub struct SvgMeshPrimitive {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub paint: SvgPaintStyle,
}

#[derive(Clone, Debug)]
pub struct SvgMeshDocument {
    pub info: SvgMeshInfo,
    pub primitives: Vec<SvgMeshPrimitive>,
}

struct SvgMeshBuilder {
    scale_x: f32,
    scale_y: f32,
    primitives: Vec<SvgMeshPrimitive>,
    fill_tess: FillTessellator,
}

struct SvgRasterizer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl SvgMeshBuilder {
    fn new(width: u32, height: u32, svg_w: f32, svg_h: f32) -> Self {
        Self {
            scale_x: width as f32 / svg_w.max(1.0),
            scale_y: height as f32 / svg_h.max(1.0),
            primitives: Vec::new(),
            fill_tess: FillTessellator::new(),
        }
    }

    fn tessellate_group(&mut self, group: &Group, inherited_opacity: f32) {
        let group_opacity = inherited_opacity * group.opacity().get();
        if group_opacity <= 0.0 {
            return;
        }

        for node in group.children() {
            match node {
                Node::Group(group) => self.tessellate_group(group, group_opacity),
                Node::Path(path) => {
                    if !path.is_visible() {
                        continue;
                    }

                    let path_data = transform_tiny_path(
                        path.data(),
                        path.abs_transform(),
                        self.scale_x,
                        self.scale_y,
                    );
                    let Some(path_data) = path_data else {
                        continue;
                    };

                    match path.paint_order() {
                        usvg::PaintOrder::FillAndStroke => {
                            if let Some(fill) = path.fill() {
                                self.push_fill_path(
                                    &path_data,
                                    fill,
                                    path.abs_bounding_box(),
                                    group_opacity,
                                );
                            }
                            if let Some(stroke) = path.stroke() {
                                self.push_stroke_path(
                                    &path_data,
                                    stroke,
                                    path.abs_stroke_bounding_box(),
                                    group_opacity,
                                );
                            }
                        }
                        usvg::PaintOrder::StrokeAndFill => {
                            if let Some(stroke) = path.stroke() {
                                self.push_stroke_path(
                                    &path_data,
                                    stroke,
                                    path.abs_stroke_bounding_box(),
                                    group_opacity,
                                );
                            }
                            if let Some(fill) = path.fill() {
                                self.push_fill_path(
                                    &path_data,
                                    fill,
                                    path.abs_bounding_box(),
                                    group_opacity,
                                );
                            }
                        }
                    }
                }
                Node::Text(_) => {}
                Node::Image(_) => {}
            }
        }
    }

    fn push_fill_path(
        &mut self,
        path: &TinyPath,
        fill: &usvg::Fill,
        abs_bbox: tiny_skia_path::Rect,
        inherited_opacity: f32,
    ) {
        let paint = build_paint_style(
            fill.paint(),
            abs_bbox,
            self.scale_x,
            self.scale_y,
            inherited_opacity * fill.opacity().get(),
        );
        let Some(paint) = paint else {
            return;
        };

        let lyon_path = tiny_path_to_lyon(path);
        let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        let mut options = FillOptions::default();
        options.fill_rule = match fill.rule() {
            usvg::FillRule::NonZero => LyonFillRule::NonZero,
            usvg::FillRule::EvenOdd => LyonFillRule::EvenOdd,
        };
        if self
            .fill_tess
            .tessellate_path(
                &lyon_path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                    [v.position().x, v.position().y]
                }),
            )
            .is_ok()
        {
            self.primitives.push(SvgMeshPrimitive {
                vertices: buffers.vertices,
                indices: buffers.indices,
                paint,
            });
        }
    }

    fn push_stroke_path(
        &mut self,
        path: &TinyPath,
        stroke: &Stroke,
        abs_bbox: tiny_skia_path::Rect,
        inherited_opacity: f32,
    ) {
        let paint = build_paint_style(
            stroke.paint(),
            abs_bbox,
            self.scale_x,
            self.scale_y,
            inherited_opacity * stroke.opacity().get(),
        );
        let Some(paint) = paint else {
            return;
        };

        let mut stroke_style = stroke.to_tiny_skia();
        stroke_style.width *= scaled_width(self.scale_x, self.scale_y);

        let Some(stroked) = path.stroke(&stroke_style, 1.0) else {
            return;
        };

        let lyon_path = tiny_path_to_lyon(&stroked);
        let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        if self
            .fill_tess
            .tessellate_path(
                &lyon_path,
                &FillOptions::default(),
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                    [v.position().x, v.position().y]
                }),
            )
            .is_ok()
        {
            self.primitives.push(SvgMeshPrimitive {
                vertices: buffers.vertices,
                indices: buffers.indices,
                paint,
            });
        }
    }
}

impl SvgRasterizer {
    fn new(width: u32, height: u32) -> Self {
        let len = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        Self {
            width,
            height,
            pixels: vec![0; len],
        }
    }

    fn rasterize_document(&mut self, doc: &SvgMeshDocument) {
        for primitive in &doc.primitives {
            self.rasterize_mesh(
                primitive.vertices.as_slice(),
                primitive.indices.as_slice(),
                &primitive.paint,
            );
        }
    }

    fn rasterize_mesh(&mut self, vertices: &[[f32; 2]], indices: &[u32], paint: &SvgPaintStyle) {
        for tri in indices.chunks_exact(3) {
            let Some(p0) = vertices.get(tri[0] as usize) else {
                continue;
            };
            let Some(p1) = vertices.get(tri[1] as usize) else {
                continue;
            };
            let Some(p2) = vertices.get(tri[2] as usize) else {
                continue;
            };
            self.rasterize_triangle(*p0, *p1, *p2, paint);
        }
    }

    fn rasterize_triangle(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        paint: &SvgPaintStyle,
    ) {
        let min_x = floorf(p0[0].min(p1[0]).min(p2[0])).max(0.0) as i32;
        let min_y = floorf(p0[1].min(p1[1]).min(p2[1])).max(0.0) as i32;
        let max_x = p0[0].max(p1[0]).max(p2[0]);
        let max_y = p0[1].max(p1[1]).max(p2[1]);
        let max_x = ceilf(max_x).min(self.width as f32) as i32;
        let max_y = ceilf(max_y).min(self.height as f32) as i32;

        if min_x >= max_x || min_y >= max_y {
            return;
        }

        let area = edge(p0, p1, p2);
        if area.abs() <= f32::EPSILON {
            return;
        }

        for y in min_y..max_y {
            for x in min_x..max_x {
                let sample = [x as f32 + 0.5, y as f32 + 0.5];
                let w0 = edge(p1, p2, sample);
                let w1 = edge(p2, p0, sample);
                let w2 = edge(p0, p1, sample);
                let inside = if area >= 0.0 {
                    w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
                } else {
                    w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
                };
                if !inside {
                    continue;
                }
                let rgba = sample_paint(paint, sample[0], sample[1]);
                if rgba[3] <= 0.0 {
                    continue;
                }
                self.blend_pixel(x as u32, y as u32, rgba);
            }
        }
    }

    fn blend_pixel(&mut self, x: u32, y: u32, src: [f32; 4]) {
        let idx = ((y as usize)
            .saturating_mul(self.width as usize)
            .saturating_add(x as usize))
        .saturating_mul(4);
        if idx + 3 >= self.pixels.len() {
            return;
        }

        let dst_r = self.pixels[idx] as f32 / 255.0;
        let dst_g = self.pixels[idx + 1] as f32 / 255.0;
        let dst_b = self.pixels[idx + 2] as f32 / 255.0;
        let dst_a = self.pixels[idx + 3] as f32 / 255.0;

        let src_a = src[3].clamp(0.0, 1.0);
        let out_a = src_a + dst_a * (1.0 - src_a);
        if out_a <= 0.0 {
            self.pixels[idx] = 0;
            self.pixels[idx + 1] = 0;
            self.pixels[idx + 2] = 0;
            self.pixels[idx + 3] = 0;
            return;
        }

        let out_r = (src[0] * src_a + dst_r * dst_a * (1.0 - src_a)) / out_a;
        let out_g = (src[1] * src_a + dst_g * dst_a * (1.0 - src_a)) / out_a;
        let out_b = (src[2] * src_a + dst_b * dst_a * (1.0 - src_a)) / out_a;

        self.pixels[idx] = to_u8(out_r);
        self.pixels[idx + 1] = to_u8(out_g);
        self.pixels[idx + 2] = to_u8(out_b);
        self.pixels[idx + 3] = to_u8(out_a);
    }
}

pub fn upload_svg_bytes_to_texture(tex_id: u32, bytes: &[u8]) -> Result<SvgTextureInfo, i32> {
    let svg_text = str::from_utf8(bytes).map_err(|_| ERR_SVG_INVALID_UTF8)?;
    upload_svg_text_to_texture(tex_id, svg_text)
}

pub fn tessellate_svg_bytes(bytes: &[u8]) -> Result<SvgMeshDocument, i32> {
    let svg_text = str::from_utf8(bytes).map_err(|_| ERR_SVG_INVALID_UTF8)?;
    tessellate_svg_text(svg_text)
}

pub fn tessellate_svg_bytes_at_height(
    bytes: &[u8],
    target_height: u32,
) -> Result<SvgMeshDocument, i32> {
    let svg_text = str::from_utf8(bytes).map_err(|_| ERR_SVG_INVALID_UTF8)?;
    tessellate_svg_text_at_height(svg_text, target_height)
}

pub fn rasterize_svg_bytes_rgba(bytes: &[u8]) -> Result<(SvgTextureInfo, Vec<u8>), i32> {
    let svg_text = str::from_utf8(bytes).map_err(|_| ERR_SVG_INVALID_UTF8)?;
    rasterize_svg_text_rgba(svg_text)
}

pub fn tessellate_svg_text(svg_text: &str) -> Result<SvgMeshDocument, i32> {
    let tree = Tree::from_str(svg_text, &Options::default()).map_err(|_| ERR_SVG_PARSE)?;
    let (width, height, svg_w, svg_h) = choose_output_size(&tree)?;
    tessellate_svg_tree_with_size(&tree, width, height, svg_w, svg_h)
}

pub fn tessellate_svg_text_at_height(
    svg_text: &str,
    target_height: u32,
) -> Result<SvgMeshDocument, i32> {
    let tree = Tree::from_str(svg_text, &Options::default()).map_err(|_| ERR_SVG_PARSE)?;
    let (_, _, svg_w, svg_h) = choose_output_size(&tree)?;
    let (width, height) = choose_mesh_size_for_height(svg_w, svg_h, target_height)?;
    tessellate_svg_tree_with_size(&tree, width, height, svg_w, svg_h)
}

fn tessellate_svg_tree_with_size(
    tree: &Tree,
    width: u32,
    height: u32,
    svg_w: f32,
    svg_h: f32,
) -> Result<SvgMeshDocument, i32> {
    let mut builder = SvgMeshBuilder::new(width, height, svg_w, svg_h);
    builder.tessellate_group(tree.root(), 1.0);
    Ok(SvgMeshDocument {
        info: SvgMeshInfo {
            width,
            height,
            svg_width: svg_w,
            svg_height: svg_h,
        },
        primitives: builder.primitives,
    })
}

fn choose_mesh_size_for_height(
    svg_w: f32,
    svg_h: f32,
    target_height: u32,
) -> Result<(u32, u32), i32> {
    if !(svg_w > 0.0 && svg_h > 0.0) {
        return Err(ERR_SVG_SIZE);
    }

    let mut height = target_height.max(1).min(SVG_MAX_TEXTURE_SIDE);
    let aspect = svg_w / svg_h;
    let mut width = floorf((height as f32 * aspect).max(1.0) + 0.5) as u32;
    if width == 0 {
        width = 1;
    }
    if width > SVG_MAX_TEXTURE_SIDE {
        let scale = SVG_MAX_TEXTURE_SIDE as f32 / width as f32;
        width = SVG_MAX_TEXTURE_SIDE;
        height = floorf((height as f32 * scale).max(1.0) + 0.5) as u32;
    }

    Ok((width.max(1), height.max(1)))
}

pub fn rasterize_svg_text_rgba(svg_text: &str) -> Result<(SvgTextureInfo, Vec<u8>), i32> {
    let mesh = tessellate_svg_text(svg_text)?;
    let mut raster = SvgRasterizer::new(mesh.info.width, mesh.info.height);
    raster.rasterize_document(&mesh);
    Ok((
        SvgTextureInfo {
            width: mesh.info.width,
            height: mesh.info.height,
        },
        raster.pixels,
    ))
}

pub fn upload_svg_text_to_texture(tex_id: u32, svg_text: &str) -> Result<SvgTextureInfo, i32> {
    let (info, rgba) = rasterize_svg_text_rgba(svg_text)?;

    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba_image(
            tex_id,
            info.width,
            info.height,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        return Err(if rc == 0 { ERR_SVG_UPLOAD } else { rc });
    }

    Ok(info)
}

fn choose_output_size(tree: &Tree) -> Result<(u32, u32, f32, f32), i32> {
    let size = tree.size();
    let svg_w = size.width();
    let svg_h = size.height();
    if !(svg_w > 0.0 && svg_h > 0.0) {
        return Err(ERR_SVG_SIZE);
    }

    let mut width = ceilf(svg_w).max(1.0) as u32;
    let mut height = ceilf(svg_h).max(1.0) as u32;
    if width > SVG_MAX_TEXTURE_SIDE || height > SVG_MAX_TEXTURE_SIDE {
        let scale = (SVG_MAX_TEXTURE_SIDE as f32 / width as f32)
            .min(SVG_MAX_TEXTURE_SIDE as f32 / height as f32);
        width = floorf(((width as f32) * scale).max(1.0) + 0.5) as u32;
        height = floorf(((height as f32) * scale).max(1.0) + 0.5) as u32;
    }

    Ok((width.max(1), height.max(1), svg_w, svg_h))
}

fn build_paint_style(
    paint: &Paint,
    abs_bbox: tiny_skia_path::Rect,
    scale_x: f32,
    scale_y: f32,
    opacity: f32,
) -> Option<SvgPaintStyle> {
    if opacity <= 0.0 {
        return None;
    }

    match paint {
        Paint::Color(color) => Some(SvgPaintStyle::Solid {
            rgba: [
                color.red as f32 / 255.0,
                color.green as f32 / 255.0,
                color.blue as f32 / 255.0,
                opacity.clamp(0.0, 1.0),
            ],
        }),
        Paint::LinearGradient(gradient) => {
            let stops = collect_gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return None;
            }
            let p0 = scale_xy(
                apply_transform_xy([gradient.x1(), gradient.y1()], gradient.transform(), abs_bbox),
                scale_x,
                scale_y,
            );
            let p1 = scale_xy(
                apply_transform_xy([gradient.x2(), gradient.y2()], gradient.transform(), abs_bbox),
                scale_x,
                scale_y,
            );
            Some(SvgPaintStyle::Linear {
                p0,
                p1,
                spread: gradient.spread_method(),
                stops,
            })
        }
        Paint::RadialGradient(gradient) => {
            let stops = collect_gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return None;
            }
            let center = scale_xy(
                apply_transform_xy([gradient.cx(), gradient.cy()], gradient.transform(), abs_bbox),
                scale_x,
                scale_y,
            );
            let radius = gradient.r().get() * scaled_width(scale_x, scale_y);
            Some(SvgPaintStyle::Radial {
                center,
                radius: radius.max(1.0),
                spread: gradient.spread_method(),
                stops,
            })
        }
        Paint::Pattern(_) => None,
    }
}

fn collect_gradient_stops(stops: &[usvg::Stop], opacity: f32) -> Vec<SvgGradientStop> {
    let mut out = Vec::with_capacity(stops.len().max(2));
    for stop in stops {
        out.push(SvgGradientStop {
            offset: stop.offset().get(),
            rgba: [
                stop.color().red as f32 / 255.0,
                stop.color().green as f32 / 255.0,
                stop.color().blue as f32 / 255.0,
                (stop.opacity().get() * opacity).clamp(0.0, 1.0),
            ],
        });
    }
    out.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    out
}

fn apply_transform_xy(p: [f32; 2], ts: TinyTransform, abs_bbox: tiny_skia_path::Rect) -> [f32; 2] {
    let bbox_origin = [abs_bbox.x(), abs_bbox.y()];
    let px = p[0] + bbox_origin[0];
    let py = p[1] + bbox_origin[1];
    [
        ts.sx * px + ts.kx * py + ts.tx,
        ts.ky * px + ts.sy * py + ts.ty,
    ]
}

fn scale_xy(p: [f32; 2], scale_x: f32, scale_y: f32) -> [f32; 2] {
    [p[0] * scale_x, p[1] * scale_y]
}

fn scaled_width(scale_x: f32, scale_y: f32) -> f32 {
    (scale_x.abs() + scale_y.abs()) * 0.5
}

fn transform_tiny_path(
    path: &TinyPath,
    ts: TinyTransform,
    scale_x: f32,
    scale_y: f32,
) -> Option<TinyPath> {
    let mut builder = TinyPathBuilder::new();

    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                let p = transform_point(p, ts, scale_x, scale_y);
                builder.move_to(p.x, p.y);
            }
            PathSegment::LineTo(p) => {
                let p = transform_point(p, ts, scale_x, scale_y);
                builder.line_to(p.x, p.y);
            }
            PathSegment::QuadTo(p1, p) => builder.quad_to(
                transform_point(p1, ts, scale_x, scale_y).x,
                transform_point(p1, ts, scale_x, scale_y).y,
                transform_point(p, ts, scale_x, scale_y).x,
                transform_point(p, ts, scale_x, scale_y).y,
            ),
            PathSegment::CubicTo(p1, p2, p) => builder.cubic_to(
                transform_point(p1, ts, scale_x, scale_y).x,
                transform_point(p1, ts, scale_x, scale_y).y,
                transform_point(p2, ts, scale_x, scale_y).x,
                transform_point(p2, ts, scale_x, scale_y).y,
                transform_point(p, ts, scale_x, scale_y).x,
                transform_point(p, ts, scale_x, scale_y).y,
            ),
            PathSegment::Close => builder.close(),
        }
    }

    builder.finish()
}

fn transform_point(p: TinyPoint, ts: TinyTransform, scale_x: f32, scale_y: f32) -> TinyPoint {
    TinyPoint::from_xy(
        (ts.sx * p.x + ts.kx * p.y + ts.tx) * scale_x,
        (ts.ky * p.x + ts.sy * p.y + ts.ty) * scale_y,
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

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])
}

fn sample_paint(paint: &SvgPaintStyle, x: f32, y: f32) -> [f32; 4] {
    match paint {
        SvgPaintStyle::Solid { rgba } => *rgba,
        SvgPaintStyle::Linear {
            p0,
            p1,
            spread,
            stops,
        } => {
            let dx = p1[0] - p0[0];
            let dy = p1[1] - p0[1];
            let len2 = dx * dx + dy * dy;
            if len2 <= f32::EPSILON {
                return stops.last().map(|s| s.rgba).unwrap_or([0.0; 4]);
            }
            let t = ((x - p0[0]) * dx + (y - p0[1]) * dy) / len2;
            sample_stops(stops, spread_t(*spread, t))
        }
        SvgPaintStyle::Radial {
            center,
            radius,
            spread,
            stops,
        } => {
            if *radius <= f32::EPSILON {
                return stops.last().map(|s| s.rgba).unwrap_or([0.0; 4]);
            }
            let dx = x - center[0];
            let dy = y - center[1];
            let t = sqrtf(dx * dx + dy * dy) / *radius;
            sample_stops(stops, spread_t(*spread, t))
        }
    }
}

fn spread_t(spread: SpreadMethod, t: f32) -> f32 {
    match spread {
        SpreadMethod::Pad => t.clamp(0.0, 1.0),
        SpreadMethod::Repeat => rem_euclid_f32(t, 1.0),
        SpreadMethod::Reflect => {
            let twice = rem_euclid_f32(t, 2.0);
            if twice > 1.0 { 2.0 - twice } else { twice }
        }
    }
}

fn sample_stops(stops: &[SvgGradientStop], t: f32) -> [f32; 4] {
    if stops.is_empty() {
        return [0.0; 4];
    }
    if t <= stops[0].offset {
        return stops[0].rgba;
    }

    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = (b.offset - a.offset).max(f32::EPSILON);
            let f = ((t - a.offset) / span).clamp(0.0, 1.0);
            return [
                lerp(a.rgba[0], b.rgba[0], f),
                lerp(a.rgba[1], b.rgba[1], f),
                lerp(a.rgba[2], b.rgba[2], f),
                lerp(a.rgba[3], b.rgba[3], f),
            ];
        }
    }

    stops.last().map(|s| s.rgba).unwrap_or([0.0; 4])
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    if modulus == 0.0 {
        return 0.0;
    }
    let div = floorf(value / modulus);
    value - div * modulus
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}
